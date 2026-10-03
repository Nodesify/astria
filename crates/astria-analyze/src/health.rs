// health: the analyst pass — code-health signals over the structural
// graph. `diagnose` audits graph *integrity* (dangling edges, stubs);
// `health` audits the *code* the graph describes: unreachable symbols,
// circular file dependencies, hub concentration, and graph staleness.
// Every signal is a heuristic over static data — the report says so.

use petgraph::algo::kosaraju_scc;
use petgraph::graph::DiGraph;
use rusqlite::Connection;
use std::collections::HashMap;

/// Dead-code candidate caps and scoring weights — a report that lists 400
/// unreachable symbols is noise; the top offenders by outgoing degree are
/// the actionable ones.
const MAX_DEAD_CODE: usize = 15;
const MAX_CYCLES: usize = 5;
const MAX_HUBS: usize = 5;

/// Symbols a static graph can never prove unreachable: entry points,
/// framework callbacks, and lifecycle hooks are invoked from outside the
/// analyzed languages.
const ENTRY_LABELS: &[&str] = &[
    "main",
    "index",
    "run",
    "mod",
    "__init__",
    "app",
    "server",
    "cli",
    "bin",
    "lib",
    "new",
    "setup",
    "handler",
    "serve",
    "start",
    "install",
    "init",
    "plugin",
    "middleware",
    "route",
];

/// Relations that carry NO usage semantics: containment (`contains` — every
/// file "reaches" the symbols it defines), co-occurrence (similarity,
/// hyperedge membership), and merge lineage. Counting them as reachability
/// would make every definition reachable simply because its file contains
/// it, and every file a hub because it has many members. Unknown future
/// relations stay counted (usage) — the conservative direction: a symbol
/// wrongly "reachable" is a missed hint, a wrongly "dead" one is a false
/// accusation.
const NON_USAGE_RELATIONS: &[&str] = &[
    "contains",
    "similar_to",
    "relates_to",
    "shares_reference",
    "participate_in",
    "rationale_for",
    "forks",
];

/// A hub must clear BOTH this floor and the graph's own 95th-percentile
/// usage degree. The floor keeps small graphs from calling every connector
/// a hub; the percentile keeps dense-but-uniform graphs from flagging their
/// ordinary top connectors.
const HUB_DEGREE_FLOOR: usize = 10;
const HUB_DEGREE_PERCENTILE: f64 = 0.95;

/// SQL fragment selecting only usage edges (see NON_USAGE_RELATIONS).
fn usage_edge_clause() -> String {
    format!(
        "relation NOT IN ({})",
        NON_USAGE_RELATIONS
            .iter()
            .map(|r| format!("'{r}'"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Largest usage degree (over NON_USAGE-filtered edges) per node id.
fn usage_degrees(db: &Connection) -> astria_core::Result<HashMap<String, usize>> {
    let mut degrees: HashMap<String, usize> = HashMap::new();
    let mut add_side = |sql: &str| -> astria_core::Result<()> {
        let mut stmt = db.prepare(sql)?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?;
        for (id, count) in rows.flatten() {
            *degrees.entry(id).or_insert(0) += count;
        }
        Ok(())
    };
    add_side(&format!(
        "SELECT target, COUNT(*) FROM edges WHERE {} GROUP BY target",
        usage_edge_clause()
    ))?;
    add_side(&format!(
        "SELECT source, COUNT(*) FROM edges WHERE {} GROUP BY source",
        usage_edge_clause()
    ))?;
    Ok(degrees)
}

/// Nearest-rank percentile of a non-empty sorted slice.
fn percentile(sorted: &[usize], p: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

#[derive(Debug, Clone)]
pub struct DeadCodeCandidate {
    pub id: String,
    pub label: String,
    pub source_file: String,
    /// Outgoing references — symbols that reach others but are never
    /// reached themselves.
    pub outgoing: usize,
}

#[derive(Debug, Clone)]
pub struct FileCycle {
    pub files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HubChurn {
    pub label: String,
    pub degree: usize,
    pub community: Option<String>,
}

#[derive(Debug)]
pub struct HealthReport {
    /// 0-100 heuristic score; deductions are documented in `health()`.
    pub score: u32,
    pub dead_code_candidates: Vec<DeadCodeCandidate>,
    pub cycles: Vec<FileCycle>,
    pub hub_churn: Vec<HubChurn>,
    /// The usage-degree threshold the flagged hubs had to clear — stated in
    /// the report so "no hubs" is readable as "nothing abnormal", not
    /// "nothing checked".
    pub hub_threshold: usize,
    /// Hub-shaped nodes in test/spec files, excluded from `hub_churn`.
    /// Fixtures are intentionally hub-shaped; they are reported, not scored.
    pub test_hubs_skipped: usize,
    /// Days since the last completed pipeline run, when one exists.
    pub age_days: Option<u64>,
    pub node_count: usize,
    pub edge_count: usize,
}

/// Symbol names that never count as dead code, after stripping call
/// parentheses and leading path dots.
fn is_entry_label(raw: &str) -> bool {
    let bare = raw
        .trim()
        .trim_end_matches("()")
        .trim_start_matches('.')
        .trim_end_matches("()")
        .to_lowercase();
    ENTRY_LABELS.contains(&bare.as_str())
}

/// Test/bench/example files intentionally expose un-referenced helpers.
fn is_test_file(path: &str) -> bool {
    let p = path.to_lowercase().replace('\\', "/");
    ["test", "spec", "example", "fixture", "benchmark"]
        .iter()
        .any(|marker| {
            p.contains(&format!("/{marker}"))
                || p.contains(&format!("{marker}s/"))
                || p.contains(&format!("_{marker}."))
        })
}

/// File-shaped node ("pipeline.rs", "src/lib.py"): files are reached by
/// being imported or contained, never by being called, so they always look
/// unreachable to a call-graph heuristic and must be excluded.
fn is_file_shaped_label(raw: &str) -> bool {
    let bare = raw.trim().trim_end_matches("()");
    match bare.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 5
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && !stem.contains('(')
        }
        None => false,
    }
}

pub fn health(db: &Connection) -> astria_core::Result<HealthReport> {
    let node_count: usize = db
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0) as usize;
    let edge_count: usize = db
        .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0) as usize;

    let dead_code_candidates = dead_code(db)?;
    let cycles = file_cycles(db)?;
    let (hub_churn, hub_threshold, test_hubs_skipped) = hub_churn(db)?;
    let age_days = graph_age_days(db);

    // Honest deduction schedule, stated in the rendered report: unreachable
    // symbols are the cheapest smell, cycles the most expensive.
    let mut score: i32 = 100;
    score -= ((dead_code_candidates.len() as i32) * 2).min(20);
    score -= ((cycles.len() as i32) * 10).min(30);
    score -= ((hub_churn.len() as i32) * 3).min(15);
    score -= age_days.map(|d| (d as i32).min(15)).unwrap_or(0);
    let score = score.clamp(0, 100) as u32;

    Ok(HealthReport {
        score,
        dead_code_candidates,
        cycles,
        hub_churn,
        hub_threshold,
        test_hubs_skipped,
        age_days,
        node_count,
        edge_count,
    })
}

/// Code symbols with outgoing usage references and zero incoming ones.
/// Degrees count only usage edges — a `contains` edge (file holds symbol)
/// is structural bookkeeping, not a reference: counting it would make every
/// definition "reachable" because its own file contains it. Deliberately
/// conservative otherwise: entry-point names and test/spec files are
/// excluded, because a static graph cannot see dynamic dispatch or
/// external invocation.
fn dead_code(db: &Connection) -> astria_core::Result<Vec<DeadCodeCandidate>> {
    // Incoming and outgoing counted separately over usage edges only: a
    // `contains` edge (file holds symbol) is structural bookkeeping, not a
    // reference — counting it would make every definition "reachable"
    // because its own file contains it.
    let indegree: HashMap<String, usize> = {
        let mut stmt = db.prepare(&format!(
            "SELECT target, COUNT(*) FROM edges WHERE {} GROUP BY target",
            usage_edge_clause()
        ))?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };
    let outdegree: HashMap<String, usize> = {
        let mut stmt = db.prepare(&format!(
            "SELECT source, COUNT(*) FROM edges WHERE {} GROUP BY source",
            usage_edge_clause()
        ))?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let mut stmt = db.prepare(
        "SELECT id, label, source_file FROM nodes
         WHERE file_type = 'code'
         ORDER BY degree_centrality DESC, id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;

    let mut candidates: Vec<DeadCodeCandidate> = Vec::new();
    for row in rows.filter_map(|r| r.ok()) {
        let (id, label, source_file) = row;
        let incoming = indegree.get(&id).copied().unwrap_or(0);
        let outgoing = outdegree.get(&id).copied().unwrap_or(0);
        if incoming > 0 || outgoing == 0 {
            continue;
        }
        if is_entry_label(&label) || is_test_file(&source_file) || is_file_shaped_label(&label) {
            continue;
        }
        candidates.push(DeadCodeCandidate {
            id,
            label,
            source_file,
            outgoing,
        });
        if candidates.len() >= MAX_DEAD_CODE {
            break;
        }
    }
    Ok(candidates)
}

/// Strongly-connected file groups over `calls`/`imports` edges that cross
/// file boundaries — the import cycles `--detail high` reviews hate.
///
/// Only EXTRACTED edges participate: INFERRED call edges are reconstructed
/// from name references and their direction is not reliable (a guard call
/// like `if !model_cached()` infers `model_cached -> caller`). An 82-file
/// "cycle" spanning unrelated crates was one such artifact — SCCs must be
/// claimed only from edges the source actually contains.
fn file_cycles(db: &Connection) -> astria_core::Result<Vec<FileCycle>> {
    let file_graph: Vec<(String, String)> = {
        let mut stmt = db.prepare(
            "SELECT DISTINCT nf.source_file, nt.source_file
             FROM edges e
             JOIN nodes nf ON nf.id = e.source
             JOIN nodes nt ON nt.id = e.target
             WHERE nf.source_file != nt.source_file
               AND e.relation IN ('calls', 'imports')
               AND e.confidence = 'EXTRACTED'",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let mut graph = DiGraph::<String, ()>::new();
    let mut idx: HashMap<String, petgraph::graph::NodeIndex> = HashMap::new();
    let node_index = |graph: &mut DiGraph<String, ()>,
                      idx: &mut HashMap<String, petgraph::graph::NodeIndex>,
                      f: &String|
     -> petgraph::graph::NodeIndex {
        *idx.entry(f.clone())
            .or_insert_with(|| graph.add_node(f.clone()))
    };
    for (from, to) in &file_graph {
        let a = node_index(&mut graph, &mut idx, from);
        let b = node_index(&mut graph, &mut idx, to);
        graph.add_edge(a, b, ());
    }

    let mut cycles: Vec<FileCycle> = kosaraju_scc(&graph)
        .into_iter()
        .filter(|scc| scc.len() > 1)
        .map(|scc| FileCycle {
            files: {
                let mut files: Vec<String> = scc.iter().map(|i| graph[*i].clone()).collect();
                files.sort();
                files
            },
        })
        .collect();
    cycles.sort_by(|a, b| {
        b.files
            .len()
            .cmp(&a.files.len())
            .then_with(|| a.files.cmp(&b.files))
    });
    cycles.truncate(MAX_CYCLES);
    Ok(cycles)
}

/// The genuinely abnormal connectors: nodes whose USAGE degree (containment
/// and co-occurrence excluded) clears both the absolute floor and the
/// graph's own 95th-percentile degree. Returns `(hubs, threshold,
/// test_hubs_skipped)` — test/spec files are intentionally hub-shaped, so
/// their candidates are counted and reported rather than scored.
fn hub_churn(db: &Connection) -> astria_core::Result<(Vec<HubChurn>, usize, usize)> {
    let community_labels: HashMap<i64, String> = {
        let mut stmt = match db.prepare("SELECT id, label FROM communities") {
            Ok(stmt) => stmt,
            Err(_) => return Ok((Vec::new(), HUB_DEGREE_FLOOR, 0)),
        };
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    let degrees = usage_degrees(db)?;

    let mut stmt = db.prepare("SELECT id, label, source_file, community FROM nodes")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<i64>>(3)?,
        ))
    })?;
    struct Candidate {
        label: String,
        degree: usize,
        community: Option<i64>,
        test_file: bool,
    }
    let mut candidates: Vec<Candidate> = rows
        .flatten()
        .map(|(id, label, source_file, community)| Candidate {
            degree: degrees.get(&id).copied().unwrap_or(0),
            test_file: is_test_file(&source_file),
            label,
            community,
        })
        .collect();
    candidates.sort_by(|a, b| b.degree.cmp(&a.degree).then_with(|| a.label.cmp(&b.label)));

    // Threshold from this graph's own distribution: a hub is abnormal
    // relative to the graph, not merely in the top five.
    let mut nonzero: Vec<usize> = candidates
        .iter()
        .map(|c| c.degree)
        .filter(|d| *d > 0)
        .collect();
    nonzero.sort_unstable();
    let threshold = HUB_DEGREE_FLOOR.max(percentile(&nonzero, HUB_DEGREE_PERCENTILE));

    let mut hubs: Vec<HubChurn> = Vec::new();
    let mut test_skipped = 0usize;
    for candidate in &candidates {
        if candidate.degree < threshold {
            break;
        }
        if candidate.test_file {
            test_skipped += 1;
            continue;
        }
        if hubs.len() >= MAX_HUBS {
            break;
        }
        hubs.push(HubChurn {
            label: candidate.label.clone(),
            degree: candidate.degree,
            community: candidate.community.map(|c| {
                community_labels
                    .get(&c)
                    .cloned()
                    .unwrap_or_else(|| c.to_string())
            }),
        });
    }
    Ok((hubs, threshold, test_skipped))
}

fn graph_age_days(db: &Connection) -> Option<u64> {
    let finished: String = db
        .query_row(
            "SELECT finished_at FROM pipeline_runs WHERE status = 'completed'
             ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok()?;
    let built = finished.parse::<u64>().ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(now.saturating_sub(built) / 86_400)
}

/// Markdown rendering. `root` (the project dir, when known) shortens file
/// paths; heuristics are stated inline so no reader treats the score as
/// ground truth.
pub fn render(report: &HealthReport, root: Option<&str>) -> String {
    let grade = match report.score {
        85..=100 => "good",
        70..=84 => "fair",
        _ => "needs attention",
    };
    let mut out = String::new();
    out.push_str(&format!(
        "# Graph Health\n\n**Score: {}/100 ({grade})** — heuristic deductions: 2/unreachable symbol (max 20), 10/file cycle (max 30), 3/hub (max 15), 1/day stale (max 15).\n\n",
        report.score
    ));
    let rel = |path: &str| -> String {
        match root {
            Some(r) => astria_paths::relative_display(path, r),
            None => path.to_string(),
        }
    };

    out.push_str(&format!(
        "## Unreachable-symbol candidates ({})\n\n",
        report.dead_code_candidates.len()
    ));
    if report.dead_code_candidates.is_empty() {
        out.push_str("None found. (Static heuristics cannot see dynamic dispatch, re-exports, or external entry points.)\n\n");
    } else {
        for c in &report.dead_code_candidates {
            out.push_str(&format!(
                "- **{}** ({}) — {} outgoing, 0 incoming\n",
                c.label,
                rel(&c.source_file),
                c.outgoing
            ));
        }
        out.push('\n');
    }

    out.push_str(&format!("## File cycles ({})\n\n", report.cycles.len()));
    if report.cycles.is_empty() {
        out.push_str("No circular dependencies detected over calls/imports.\n\n");
    } else {
        for cycle in &report.cycles {
            out.push_str(&format!(
                "- {} files: {}\n",
                cycle.files.len(),
                cycle
                    .files
                    .iter()
                    .map(|f| rel(f))
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ));
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "## Hub concentration ({})\n\n",
        report.hub_churn.len()
    ));
    if report.hub_churn.is_empty() {
        out.push_str(&format!(
            "No abnormal hubs (flagged only above usage-degree {}, containment excluded).\n\n",
            report.hub_threshold
        ));
    } else {
        for h in &report.hub_churn {
            let community = h.community.as_deref().unwrap_or("-");
            out.push_str(&format!(
                "- **{}** (degree {}, community {community})\n",
                h.label, h.degree
            ));
        }
        out.push('\n');
    }
    if report.test_hubs_skipped > 0 {
        out.push_str(&format!(
            "({} test/spec hub-shaped node(s) excluded from scoring)\n\n",
            report.test_hubs_skipped
        ));
    }

    match report.age_days {
        Some(0) => out.push_str("## Staleness\n\nGraph built today.\n"),
        Some(days) => out.push_str(&format!(
            "## Staleness\n\nLast completed build was {days} day(s) ago — run `astria update` before trusting blast radii.\n"
        )),
        None => out.push_str("## Staleness\n\nNo completed pipeline run recorded.\n"),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    fn seed(db: &Connection, sql: &str) {
        db.execute_batch(sql).unwrap();
    }

    #[test]
    fn dead_code_found_but_entries_and_tests_excluded() {
        let db = open_db_in_memory().unwrap();
        seed(
            &db,
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('orphan', 'helper_util()', 'code', 'src/util.py'),
                ('entry', 'main()', 'code', 'src/main.py'),
                ('testhelper', 'only_in_tests()', 'code', 'tests/unit.py'),
                ('used', 'used_fn()', 'code', 'src/used.py');
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('orphan', 'entry', 'calls', 'EXTRACTED', 'src/util.py'),
                ('entry', 'used', 'calls', 'EXTRACTED', 'src/main.py'),
                ('used', 'used', 'calls', 'EXTRACTED', 'src/used.py');",
        );
        let report = health(&db).unwrap();
        assert_eq!(
            report.dead_code_candidates.len(),
            1,
            "only orphan qualifies"
        );
        assert_eq!(report.dead_code_candidates[0].label, "helper_util()");
    }

    #[test]
    fn file_cycle_detected_and_acyclic_not() {
        let db = open_db_in_memory().unwrap();
        seed(
            &db,
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a1', 'a()', 'code', 'src/a.rs'), ('b1', 'b()', 'code', 'src/b.rs'),
                ('c1', 'c()', 'code', 'src/c.rs'), ('d1', 'd()', 'code', 'src/d.rs');
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('a1', 'b1', 'calls', 'EXTRACTED', 'src/a.rs'),
                ('b1', 'a1', 'calls', 'EXTRACTED', 'src/b.rs'),
                ('c1', 'd1', 'calls', 'EXTRACTED', 'src/c.rs');",
        );
        let report = health(&db).unwrap();
        assert_eq!(
            report.cycles.len(),
            1,
            "a.rs <-> b.rs cycles; c -> d does not"
        );
        let files = &report.cycles[0].files;
        assert!(files.contains(&"src/a.rs".to_string()));
        assert!(files.contains(&"src/b.rs".to_string()));
    }

    #[test]
    fn non_structural_relations_do_not_create_cycles() {
        let db = open_db_in_memory().unwrap();
        seed(
            &db,
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a1', 'a()', 'code', 'src/a.rs'), ('b1', 'b()', 'code', 'src/b.rs');
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('a1', 'b1', 'similar_to', 'INFERRED', 'src/a.rs'),
                ('b1', 'a1', 'relates_to', 'INFERRED', 'src/b.rs');",
        );
        let report = health(&db).unwrap();
        assert!(
            report.cycles.is_empty(),
            "similarity edges are not dependencies"
        );
    }

    #[test]
    fn inferred_call_edges_do_not_create_cycles() {
        // The measured artifact: INFERRED call edges with unreliable
        // direction chained dozens of unrelated files into one SCC.
        // Only edges the source actually contains may claim a cycle.
        let db = open_db_in_memory().unwrap();
        seed(
            &db,
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a1', 'a()', 'code', 'src/a.rs'), ('b1', 'b()', 'code', 'src/b.rs');
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('a1', 'b1', 'calls', 'INFERRED', 'src/a.rs'),
                ('b1', 'a1', 'calls', 'INFERRED', 'src/b.rs');",
        );
        let report = health(&db).unwrap();
        assert!(
            report.cycles.is_empty(),
            "an INFERRED-only cycle is not a source-level dependency cycle"
        );
    }

    #[test]
    fn score_drops_with_issues_and_stays_bounded() {
        let db = open_db_in_memory().unwrap();
        seed(
            &db,
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a1', 'a()', 'code', 'src/a.rs'), ('b1', 'b()', 'code', 'src/b.rs');
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('a1', 'b1', 'calls', 'EXTRACTED', 'src/a.rs'),
                ('b1', 'a1', 'calls', 'EXTRACTED', 'src/b.rs');",
        );
        let report = health(&db).unwrap();
        assert!(report.score < 100, "a cycle must cost points");
        assert!(report.score <= 100);
        // An empty graph scores full marks.
        let empty = open_db_in_memory().unwrap();
        let clean = health(&empty).unwrap();
        assert_eq!(clean.score, 100);
    }

    #[test]
    fn render_states_heuristics_and_sections() {
        let db = open_db_in_memory().unwrap();
        seed(
            &db,
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('orphan', 'orphan_fn()', 'code', 'src/u.py'),
                ('sink', 'sink_fn()', 'code', 'src/u.py');
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('orphan', 'sink', 'calls', 'EXTRACTED', 'src/u.py');",
        );
        let report = health(&db).unwrap();
        let text = render(&report, None);
        assert!(text.contains("Graph Health"));
        assert!(text.contains("heuristic deductions"));
        assert!(text.contains("orphan_fn()"));
        assert!(text.contains("Unreachable-symbol candidates"));
    }

    #[test]
    fn entry_labels_across_shapes_are_excluded() {
        assert!(is_entry_label("main()"));
        assert!(is_entry_label(".index"));
        assert!(is_entry_label("__init__"));
        assert!(!is_entry_label("helper_util()"));
    }

    #[test]
    fn file_shaped_labels_are_never_dead_code() {
        assert!(is_file_shaped_label("pipeline.rs"));
        assert!(is_file_shaped_label("src/lib.py"));
        assert!(is_file_shaped_label("notes.md"));
        // Call paths and plain calls stay eligible.
        assert!(!is_file_shaped_label("helper_util()"));
        assert!(!is_file_shaped_label("auth::login()"));
        assert!(!is_file_shaped_label("no_extension"));
    }

    #[test]
    fn containment_edges_do_not_make_symbols_reachable() {
        // The measured artifact: a `contains` edge (file holds symbol) used
        // to count as an incoming reference, so every definition in the
        // graph looked reachable and dead-code detection found nothing.
        let db = open_db_in_memory().unwrap();
        seed(
            &db,
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('file', 'util.py', 'code', 'src/util.py'),
                ('helper', 'helper_util()', 'code', 'src/util.py'),
                ('other', 'other_fn()', 'code', 'src/other.py');
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('file', 'helper', 'contains', 'EXTRACTED', 'src/util.py'),
                ('helper', 'other', 'calls', 'EXTRACTED', 'src/util.py');",
        );
        let report = health(&db).unwrap();
        assert_eq!(
            report.dead_code_candidates.len(),
            1,
            "contains is containment, not a reference"
        );
        assert_eq!(report.dead_code_candidates[0].label, "helper_util()");
    }

    #[test]
    fn co_occurrence_edges_do_not_make_symbols_reachable() {
        let db = open_db_in_memory().unwrap();
        seed(
            &db,
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a', 'fn_a()', 'code', 'src/a.py'),
                ('b', 'fn_b()', 'code', 'src/b.py');
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('a', 'b', 'similar_to', 'SEMANTIC', 'src/a.py'),
                ('b', 'a', 'shares_reference', 'EXTRACTED', 'src/b.py');",
        );
        let report = health(&db).unwrap();
        // Both have outgoing usage? No — only co-occurrence edges exist, so
        // neither has usage degree at all: no dead-code claims either.
        assert!(
            report.dead_code_candidates.is_empty(),
            "co-occurrence is not usage in either direction"
        );
    }

    #[test]
    fn hubs_require_abnormal_degree() {
        // A uniform small graph: every connector has degree 2, far below the
        // absolute floor — the old heuristic still flagged the top five.
        let db = open_db_in_memory().unwrap();
        let mut sql = String::new();
        for i in 0..6 {
            sql.push_str(&format!(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('n{i}', 'fn_{i}()', 'code', 'src/m{i}.py');\n"
            ));
        }
        for i in 0..5 {
            sql.push_str(&format!(
                "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('n{i}', 'n{}', 'calls', 'EXTRACTED', 'src/m{i}.py');\n",
                i + 1
            ));
        }
        seed(&db, &sql);
        let report = health(&db).unwrap();
        assert!(
            report.hub_churn.is_empty(),
            "uniform low-degree graphs have no abnormal hubs"
        );
        assert!(
            report.hub_threshold >= 10,
            "threshold never drops below the floor"
        );

        // Now add one genuine outlier: 40 callers through one symbol while
        // the rest of the distribution stays low.
        let mut sql = String::new();
        for i in 0..40 {
            sql.push_str(&format!(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('x{i}', 'caller_{i}()', 'code', 'src/x{i}.py');\n"
            ));
            sql.push_str(&format!(
                "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('x{i}', 'n0', 'calls', 'EXTRACTED', 'src/x{i}.py');\n"
            ));
        }
        db.execute_batch(&sql).unwrap();
        let report = health(&db).unwrap();
        assert_eq!(report.hub_churn.len(), 1, "only the outlier is abnormal");
        assert!(report.hub_churn[0].label.starts_with("fn_0"));
    }

    #[test]
    fn test_file_hubs_are_reported_not_scored() {
        let db = open_db_in_memory().unwrap();
        let mut sql = String::new();
        for i in 0..30 {
            sql.push_str(&format!(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('p{i}', 'prod_{i}()', 'code', 'src/p{i}.py'), ('t{i}', 'fixture_{i}()', 'code', 'tests/t{i}.py');\n"
            ));
            sql.push_str(&format!(
                "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('p{i}', 't0', 'calls', 'EXTRACTED', 'src/p{i}.py');\n"
            ));
        }
        seed(&db, &sql);
        let report = health(&db).unwrap();
        // The only hub-shaped node lives in tests/ — excluded from scoring,
        // but the report says it was skipped instead of hiding it.
        assert!(report.hub_churn.is_empty());
        assert_eq!(report.test_hubs_skipped, 1);
    }

    #[test]
    fn containment_edges_do_not_create_hubs() {
        // A file with 200 contained symbols has 200 contains edges — that is
        // a container, not a hub; only usage edges may concentrate.
        let db = open_db_in_memory().unwrap();
        let mut sql = String::new();
        sql.push_str(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('big', 'big.py', 'code', 'src/big.py');\n",
        );
        for i in 0..200 {
            sql.push_str(&format!(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('s{i}', 'sym_{i}()', 'code', 'src/big.py');\n"
            ));
            sql.push_str(&format!(
                "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('big', 's{i}', 'contains', 'EXTRACTED', 'src/big.py');\n"
            ));
        }
        seed(&db, &sql);
        let report = health(&db).unwrap();
        assert!(
            report.hub_churn.iter().all(|h| h.label != "big.py"),
            "containment degree must not qualify a file as a hub"
        );
    }
}
