// graphify-report: markdown report generation

use graphify_analyze::AnalysisResult;
use rusqlite::Connection;
use std::collections::HashMap;

/// Hub-based community labels from the communities table, falling back to
/// plain numbers when missing.
fn community_labels(db: &Connection) -> HashMap<i64, String> {
    let mut stmt = match db.prepare("SELECT id, label FROM communities") {
        Ok(stmt) => stmt,
        Err(_) => return HashMap::new(),
    };
    stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

pub fn generate_report(
    db: &Connection,
    analysis: &AnalysisResult,
) -> graphify_core::Result<String> {
    let node_count: i64 = db
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .unwrap_or(0);
    let edge_count: i64 = db
        .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
        .unwrap_or(0);
    let community_count: i64 = db
        .query_row(
            "SELECT COUNT(DISTINCT community) FROM nodes WHERE community IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let labels = community_labels(db);
    let label_of = |c: Option<u32>| -> String {
        match c {
            Some(c) => labels
                .get(&(c as i64))
                .cloned()
                .unwrap_or_else(|| c.to_string()),
            None => "—".into(),
        }
    };

    let mut report = String::new();
    report.push_str("# Graph Report\n\n");
    let modularity: Option<f64> = db
        .query_row(
            "SELECT CAST(value AS REAL) FROM _meta WHERE key = 'last_modularity'",
            [],
            |r| r.get(0),
        )
        .ok();
    match modularity {
        Some(q) => report.push_str(&format!(
            "**Nodes:** {} | **Edges:** {} | **Communities:** {} | **Modularity:** {:.3}\n\n",
            node_count, edge_count, community_count, q
        )),
        None => report.push_str(&format!(
            "**Nodes:** {} | **Edges:** {} | **Communities:** {}\n\n",
            node_count, edge_count, community_count
        )),
    }

    // Which build produced this graph - lets agents (and hooks) detect a
    // stale graph written by an older binary.
    let pipeline_version: Option<String> = db
        .query_row(
            "SELECT value FROM _meta WHERE key = 'pipeline_version'",
            [],
            |r| r.get(0),
        )
        .ok();
    if let Some(v) = pipeline_version {
        report.push_str(&format!("_Built by graphify v{v}._\n\n"));
    }

    // Hyperedges: deterministic N-ary group relationships (communities,
    // shared reference groups) — a group that pairwise edges cannot express.
    if let Ok(hyperedges) = graphify_build::hyperedges::load_all(db) {
        if !hyperedges.is_empty() {
            report.push_str(&format!(
                "## Hyperedges ({} group relationships)\n\n",
                hyperedges.len()
            ));
            for h in hyperedges.iter().take(15) {
                let score = h.score.map(|s| format!(" {s:.2}")).unwrap_or_default();
                report.push_str(&format!(
                    "- **{}** — {} members [{}{}] `id: {}`\n",
                    h.label,
                    h.nodes.len(),
                    h.confidence,
                    score,
                    h.id
                ));
            }
            if hyperedges.len() > 15 {
                report.push_str(&format!(
                    "\n... and {} more (see graph.json `hyperedges`).\n",
                    hyperedges.len() - 15
                ));
            }
            report.push('\n');
        }
    }

    report.push_str("## Communities\n\n");
    {
        let communities_stmt =
            db.prepare("SELECT label, size, cohesion FROM communities ORDER BY size DESC LIMIT 10");
        if let Ok(mut stmt) = communities_stmt {
            let rows: Vec<(String, i64, Option<f64>)> = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<f64>>(2)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();
            if rows.is_empty() {
                report.push_str("No communities detected.\n\n");
            } else {
                for (label, size, cohesion) in &rows {
                    let coh = cohesion
                        .map(|c| format!("{c:.2}"))
                        .unwrap_or_else(|| "-".into());
                    report.push_str(&format!("- **{label}** ({size} nodes, cohesion {coh})\n"));
                }
                if community_count > rows.len() as i64 {
                    report.push_str(&format!(
                        "\n... and {} more communities (see the MCP list_communities tool).\n",
                        community_count - rows.len() as i64
                    ));
                }
                report.push('\n');
            }
        }
    }

    report.push_str(
        "## Key Files

",
    );
    {
        // Aggregate degree per file from two index-friendly joins; shown
        // relative to the project root. Lightweight companion to the full
        // PageRank map (`map` command / repo_map MCP tool).
        let root = db.path().and_then(|db_path| {
            std::path::Path::new(db_path)
                .parent()?
                .parent()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
        });
        let sql = "SELECT f, SUM(cnt) AS total FROM (                      SELECT n.source_file AS f, COUNT(*) AS cnt FROM nodes n JOIN edges e ON e.source = n.id GROUP BY n.source_file                      UNION ALL                      SELECT n.source_file AS f, COUNT(*) AS cnt FROM nodes n JOIN edges e ON e.target = n.id GROUP BY n.source_file                    ) GROUP BY f ORDER BY total DESC, f ASC LIMIT 10";
        let mut stmt = db.prepare(sql)?;
        let rows: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        if rows.is_empty() {
            report.push_str(
                "No connected files.

",
            );
        } else {
            for (file, total) in &rows {
                let display = match &root {
                    Some(r) => graphify_paths::relative_display(file, r),
                    None => file.clone(),
                };
                report.push_str(&format!(
                    "- **{display}** ({total} edge endpoints)
"
                ));
            }
            report.push('\n');
        }
    }

    report.push_str("## Hub Nodes (God Nodes)\n\n");
    if analysis.god_nodes.is_empty() {
        report.push_str("No hub nodes found.\n\n");
    } else {
        for node in &analysis.god_nodes {
            report.push_str(&format!(
                "- **{}** (degree: {}, community: {})\n",
                node.label,
                node.degree,
                label_of(node.community)
            ));
        }
        report.push('\n');
    }

    report.push_str("## Surprising Connections\n\n");
    if analysis.surprising_connections.is_empty() {
        report.push_str("No cross-community connections found.\n\n");
    } else {
        report.push_str(&format!(
            "Top {} of the cross-community edges, ranked by novelty (bigger, more cohesive communities joined by fewer edges score higher):\n\n",
            analysis.surprising_connections.len()
        ));
        for edge in &analysis.surprising_connections {
            report.push_str(&format!(
                "- **{}** -> **{}** ({}) [{} -> {}] (score: {:.2})\n",
                edge.source_label,
                edge.target_label,
                edge.relation,
                label_of(edge.source_community),
                label_of(edge.target_community),
                edge.score
            ));
        }
        report.push('\n');
    }

    let dedup_merged: i64 = db
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM _meta WHERE key = 'last_dedup_merged'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if dedup_merged > 0 {
        report.push_str(&format!(
            "## Merged Duplicates\n\n{} near-duplicate node(s) merged into canonical entities.\n\n",
            dedup_merged
        ));
    }

    report.push_str("## Suggested Questions\n\n");
    for q in &analysis.suggested_questions {
        report.push_str(&format!("- {}\n", q));
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphify_analyze::{NodeAnalysis, SurprisingEdge};
    use graphify_core::db::open_db_in_memory;

    #[test]
    fn generate_report_with_data() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch("
            INSERT INTO nodes (id, label, file_type, source_file, community) VALUES ('a', 'Alpha', 'code', 'f.py', 0);
            INSERT INTO nodes (id, label, file_type, source_file, community) VALUES ('b', 'Beta', 'code', 'f.py', 1);
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('a', 'b', 'calls', 'EXTRACTED', 'f.py');
        ").unwrap();

        let analysis = AnalysisResult {
            god_nodes: vec![NodeAnalysis {
                id: "a".into(),
                label: "Alpha".into(),
                degree: 1,
                community: Some(0),
                is_stub: false,
            }],
            surprising_connections: vec![SurprisingEdge {
                source: "a".into(),
                source_label: "Alpha".into(),
                target: "b".into(),
                target_label: "Beta".into(),
                relation: "calls".into(),
                source_community: Some(0),
                target_community: Some(1),
                score: 2.5,
            }],
            suggested_questions: vec!["Why does Alpha have so many connections?".into()],
        };

        let report = generate_report(&db, &analysis).unwrap();
        assert!(report.contains("# Graph Report"));
        assert!(report.contains("Alpha"));
        assert!(report.contains("**Alpha** -> **Beta** (calls)"));
        assert!(report.contains("Surprising Connections"));
        assert!(report.contains("Suggested Questions"));
    }

    #[test]
    fn generate_report_empty_graph() {
        let db = open_db_in_memory().unwrap();
        let analysis = AnalysisResult {
            god_nodes: vec![],
            surprising_connections: vec![],
            suggested_questions: vec![],
        };
        let report = generate_report(&db, &analysis).unwrap();
        assert!(report.contains("**Nodes:** 0"));
    }
}
