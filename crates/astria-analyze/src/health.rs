// health: the analyst pass — code-health signals over the structural
// graph. `diagnose` audits graph *integrity* (dangling edges, stubs);
// `health` audits the *code* the graph describes: unreachable symbols,
// circular file dependencies, hub concentration, and graph staleness.
// Every signal is a heuristic over static data — the report says so.

use crate::NodeAnalysis;
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
    let hub_churn = hub_churn(db)?;
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
        age_days,
        node_count,
        edge_count,
    })
}

/// Code symbols with outgoing references and zero incoming ones. Deliberately
/// conservative: entry-point names and test/spec files are excluded, because
/// a static graph cannot see dynamic dispatch or external invocation.
fn dead_code(db: &Connection) -> astria_core::Result<Vec<DeadCodeCandidate>> {
    let indegree: HashMap<String, i64> = {
        let mut stmt = db.prepare("SELECT target, COUNT(*) FROM edges GROUP BY target")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    let outdegree: HashMap<String, i64> = {
        let mut stmt = db.prepare("SELECT source, COUNT(*) FROM edges GROUP BY source")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
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
        let incoming = *indegree.get(&id).unwrap_or(&0);
        let outgoing = *outdegree.get(&id).unwrap_or(&0) as usize;
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

/// The god nodes, with their community's label — everything routes through
/// them, so every change nearby is a change to them.
fn hub_churn(db: &Connection) -> astria_core::Result<Vec<HubChurn>> {
    let community_labels: HashMap<i64, String> = {
        let mut stmt = match db.prepare("SELECT id, label FROM communities") {
            Ok(stmt) => stmt,
            Err(_) => return Ok(Vec::new()),
        };
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    let analysis = crate::analyze(db)?;
    let hubs: Vec<HubChurn> = analysis
        .god_nodes
        .iter()
        .take(MAX_HUBS)
        .map(
            |NodeAnalysis {
                 label,
                 degree,
                 community,
                 ..
             }| HubChurn {
                label: label.clone(),
                degree: *degree,
                community: community.map(|c| {
                    community_labels
                        .get(&(c as i64))
                        .cloned()
                        .unwrap_or_else(|| c.to_string())
                }),
            },
        )
        .collect();
    Ok(hubs)
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
        out.push_str("No hub nodes.\n\n");
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
}
