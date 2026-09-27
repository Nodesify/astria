// risk: the PR gate — blast radius of the current git diff. Changed files
// are mapped to graph nodes, `affected` runs reverse reachability per
// seed, and the union is scored so a reviewer (or CI) can triage without
// reading the whole diff. Read-only: this never mutates the graph.

use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::Path;

/// Changed files that map to no graph node cost nothing — renames of
/// untracked assets, docs-only edits — but they are still listed.
pub struct RiskOutcome {
    pub score: u32,
    pub changed_files: Vec<String>,
    /// Files that actually contain graph symbols.
    pub files_with_symbols: usize,
    pub impacted: usize,
    pub by_depth: BTreeMap<u32, usize>,
    pub communities: Vec<String>,
    pub entries: Vec<RiskEntry>,
}

pub struct RiskEntry {
    pub label: String,
    pub depth: u32,
    pub relation: String,
    pub via_file: String,
}

/// Heuristic score: each changed file carrying graph symbols is a standing
/// risk, each impacted symbol adds reach, and cross-community reach costs
/// most. Documented here and in the rendered report — it is a triage
/// signal, not a proof.
fn score_outcome(files_with_symbols: usize, impacted: usize, communities: usize) -> u32 {
    let raw = 8 * files_with_symbols + 2 * impacted + 6 * communities;
    raw.min(100) as u32
}

fn risk_level(score: u32) -> &'static str {
    match score {
        0..=29 => "low",
        30..=59 => "medium",
        _ => "high",
    }
}

/// `git diff --name-only` over the working tree (HEAD) or the index
/// (`--cached`). Untracked files are invisible to `git diff` — documented
/// in the rendered report.
pub fn git_changed_files(root: &Path, staged: bool) -> astria_core::Result<Vec<String>> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .arg("diff")
        .arg("--name-only")
        .arg(if staged { "--cached" } else { "HEAD" });
    let out = cmd.output().map_err(astria_core::AstriaError::Io)?;
    if !out.status.success() {
        return Err(astria_core::AstriaError::Graph(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().replace('\\', "/"))
        .filter(|l| !l.is_empty())
        .collect())
}

/// Union the reverse reachability of every node defined in the changed
/// files. Duplicate hits keep their minimum depth (closest cause wins).
pub fn compute_risk(db: &Connection, changed_files: &[String]) -> astria_core::Result<RiskOutcome> {
    let mut impacted_ids: std::collections::HashMap<String, (u32, String, String)> =
        std::collections::HashMap::new();
    let mut seed_ids: Vec<String> = Vec::new();
    let mut files_with_symbols = 0usize;
    let communities: Vec<String>;

    for file in changed_files {
        let seeds: Vec<(String, String)> = {
            let mut stmt = db.prepare(
                "SELECT id, label FROM nodes WHERE source_file = ?1 AND file_type = 'code'",
            )?;
            let rows = stmt.query_map(rusqlite::params![file], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.filter_map(|r| r.ok()).collect()
        };
        if seeds.is_empty() {
            continue;
        }
        files_with_symbols += 1;
        for (seed_id, _label) in &seeds {
            seed_ids.push(seed_id.clone());
            let result = match astria_analyze::affected::affected(db, seed_id, 2, None) {
                Ok(r) => r,
                // A seed that no longer resolves (mid-refactor working
                // tree) must not sink the whole report.
                Err(_) => continue,
            };
            for hit in &result.hits {
                impacted_ids.entry(hit.id.clone()).or_insert((
                    hit.depth,
                    hit.relation.clone(),
                    hit.via_file.clone(),
                ));
            }
        }
    }

    // Communities touched: the changed files' own communities count too —
    // editing auth.rs means touching Auth even when nothing else breaks.
    // One parameterized batch query per 500 ids keeps the SQL variable
    // limit far away.
    let mut touched: std::collections::BTreeMap<i64, String> = Default::default();
    {
        let mut query_ids: Vec<String> = impacted_ids.keys().cloned().collect();
        query_ids.extend(seed_ids.iter().cloned());
        let ids: Vec<&String> = query_ids.iter().collect();
        for chunk in ids.chunks(500) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT DISTINCT community, (SELECT label FROM communities c WHERE c.id = n.community)
                 FROM nodes n WHERE n.community IS NOT NULL AND n.id IN ({placeholders})"
            );
            let mut stmt = db.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
            })?;
            for row in rows.filter_map(|r| r.ok()) {
                let (community, label) = row;
                touched
                    .entry(community)
                    .or_insert_with(|| label.unwrap_or_else(|| format!("community {community}")));
            }
        }
        communities = touched.into_values().collect();
    }

    let mut by_depth: BTreeMap<u32, usize> = Default::default();
    let mut entries: Vec<RiskEntry> = Vec::new();
    for (id, (depth, relation, via_file)) in &impacted_ids {
        *by_depth.entry(*depth).or_insert(0) += 1;
        if entries.len() < 10 {
            let label = node_label(db, id);
            entries.push(RiskEntry {
                label,
                depth: *depth,
                relation: relation.clone(),
                via_file: via_file.clone(),
            });
        }
    }
    entries.sort_by_key(|e| e.depth);

    let score = score_outcome(files_with_symbols, impacted_ids.len(), communities.len());
    Ok(RiskOutcome {
        score,
        changed_files: changed_files.to_vec(),
        files_with_symbols,
        impacted: impacted_ids.len(),
        by_depth,
        communities,
        entries,
    })
}

fn node_label(db: &Connection, id: &str) -> String {
    db.query_row(
        "SELECT label FROM nodes WHERE id = ?1",
        rusqlite::params![id],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_else(|_| id.to_string())
}

pub fn level_of(score: u32) -> &'static str {
    risk_level(score)
}

/// Markdown rendering, shaped to paste straight into a PR description.
pub fn render(outcome: &RiskOutcome, root: Option<&str>) -> String {
    let rel = |path: &str| -> String {
        match root {
            Some(r) => astria_paths::relative_display(path, r),
            None => path.to_string(),
        }
    };
    let mut out = String::new();
    out.push_str(&format!(
        "# Change Risk Report\n\n**Score: {}/100 ({})** — heuristic: 8 per changed file with graph symbols, 2 per impacted symbol, 6 per community touched. A triage signal, not a proof.\n\n",
        outcome.score,
        risk_level(outcome.score)
    ));
    out.push_str(&format!(
        "## Changed files ({} tracked, {} with graph symbols)\n\n",
        outcome.changed_files.len(),
        outcome.files_with_symbols
    ));
    for f in &outcome.changed_files {
        out.push_str(&format!("- {}\n", rel(f)));
    }
    out.push('\n');

    out.push_str(&format!("## Impacted symbols ({})\n\n", outcome.impacted));
    if outcome.by_depth.is_empty() {
        out.push_str("None — the diff touches no graph-connected code.\n\n");
    } else {
        for (depth, count) in &outcome.by_depth {
            out.push_str(&format!("- depth {depth}: {count} symbol(s)\n"));
        }
        out.push('\n');
    }

    if !outcome.communities.is_empty() {
        out.push_str(&format!(
            "## Communities touched ({})\n\n",
            outcome.communities.len()
        ));
        for c in &outcome.communities {
            out.push_str(&format!("- {c}\n"));
        }
        out.push('\n');
    }

    if !outcome.entries.is_empty() {
        out.push_str("## Review focus (closest impacted first)\n\n");
        for e in &outcome.entries {
            out.push_str(&format!(
                "- **{}** (depth {}) via {} from {}\n",
                e.label,
                e.depth,
                e.relation,
                rel(&e.via_file)
            ));
        }
        out.push('\n');
    }
    out.push_str(
        "_Untracked files are invisible to `git diff`; run the graph update (`astria update`) before scoring for full coverage._\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    fn seed(db: &Connection) {
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, community) VALUES
                ('a', 'auth_login()', 'code', 'src/auth.rs', 0),
                ('b', 'session_store()', 'code', 'src/session.rs', 0),
                ('c', 'api_handler()', 'code', 'src/api.rs', 1);
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('c', 'a', 'calls', 'EXTRACTED', 'src/api.rs'),
                ('a', 'b', 'calls', 'EXTRACTED', 'src/auth.rs');
             INSERT INTO communities (id, label, size) VALUES (0, 'Auth', 2), (1, 'Api', 1);",
        )
        .unwrap();
    }

    #[test]
    fn diff_on_one_file_union_reachability() {
        let db = open_db_in_memory().unwrap();
        seed(&db);
        let outcome = compute_risk(&db, &["src/auth.rs".to_string()]).unwrap();
        // auth.rs defines a; a reaches b at depth 1. c calls a, but reverse
        // reachability from a does not include its callers.
        assert_eq!(outcome.files_with_symbols, 1);
        assert_eq!(outcome.impacted, 1);
        assert_eq!(outcome.by_depth.get(&1), Some(&1));
    }

    #[test]
    fn files_without_symbols_are_listed_but_free() {
        let db = open_db_in_memory().unwrap();
        seed(&db);
        let outcome = compute_risk(
            &db,
            &["src/auth.rs".to_string(), "docs/notes.md".to_string()],
        )
        .unwrap();
        assert_eq!(outcome.changed_files.len(), 2);
        assert_eq!(outcome.files_with_symbols, 1, "docs carry no code nodes");
        assert!(outcome.score > 0);
    }

    #[test]
    fn score_prefers_wide_and_cross_community_changes() {
        let db = open_db_in_memory().unwrap();
        seed(&db);
        // A file whose only node nothing depends on is the cheapest change.
        db.execute(
            "INSERT INTO nodes (id, label, file_type, source_file, community) VALUES ('d', 'leaf()', 'code', 'src/leaf.rs', 1)",
            [],
        )
        .unwrap();
        let narrow = compute_risk(&db, &["src/leaf.rs".to_string()]).unwrap();
        assert_eq!(narrow.impacted, 0, "leaf has no dependents");
        // auth.rs (a) + api.rs (c): their dependents span two communities.
        let wide =
            compute_risk(&db, &["src/auth.rs".to_string(), "src/api.rs".to_string()]).unwrap();
        assert!(
            wide.score > narrow.score,
            "wide cross-community change must outrank a leaf file (wide {} vs narrow {})",
            wide.score,
            narrow.score
        );
        assert!(wide.communities.len() >= 2);
        assert!(narrow.score <= 100 && wide.score <= 100);
    }

    #[test]
    fn level_thresholds() {
        assert_eq!(level_of(0), "low");
        assert_eq!(level_of(45), "medium");
        assert_eq!(level_of(90), "high");
    }

    #[test]
    fn render_lists_sections() {
        let db = open_db_in_memory().unwrap();
        seed(&db);
        let outcome = compute_risk(&db, &["src/auth.rs".to_string()]).unwrap();
        assert!(outcome.impacted > 0, "auth.rs has a caller chain");
        let text = render(&outcome, None);
        assert!(text.contains("Change Risk Report"));
        assert!(text.contains("Impacted symbols"));
        assert!(text.contains("Communities touched"));
        assert!(text.contains("git diff"));
    }
}
