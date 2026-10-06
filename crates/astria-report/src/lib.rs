// astria-report: actionable source orientation with explicit relationship evidence.
mod orientation;

use astria_analyze::AnalysisResult;
use orientation::{escape, Sources, ORIENTATIONS};
use rusqlite::{Connection, OptionalExtension};
use std::collections::{BTreeMap, HashMap, HashSet};

const MAX_RELATIONSHIPS_PER_CATEGORY: usize = 25;

fn community_labels(db: &Connection) -> astria_core::Result<HashMap<i64, String>> {
    let mut stmt = db.prepare("SELECT id, label FROM communities")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Missing metadata is optional; malformed values and failed reads are not.
fn metadata(db: &Connection, key: &str) -> astria_core::Result<Option<String>> {
    Ok(db
        .query_row("SELECT value FROM _meta WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}
fn numeric_metadata<T: std::str::FromStr>(
    db: &Connection,
    key: &str,
) -> astria_core::Result<Option<T>> {
    metadata(db, key)?
        .map(|v| {
            v.parse().map_err(|_| {
                astria_core::AstriaError::Graph(format!("Invalid numeric metadata {key}: {v}"))
            })
        })
        .transpose()
}

pub fn generate_report(db: &Connection, analysis: &AnalysisResult) -> astria_core::Result<String> {
    let node_count: i64 = db.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
    let edge_count: i64 = db.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
    let community_count: i64 = db.query_row(
        "SELECT COUNT(DISTINCT community) FROM nodes WHERE community IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let labels = community_labels(db)?;
    let sources = Sources::load(db)?;
    let label_of = |c: Option<u32>| -> String {
        c.map(|c| {
            labels
                .get(&(c as i64))
                .cloned()
                .unwrap_or_else(|| c.to_string())
        })
        .unwrap_or_else(|| "—".into())
    };
    let mut report = format!("# Graph Report\n\n**Nodes:** {node_count} | **Edges:** {edge_count} | **Communities:** {community_count}");
    if let Some(q) = numeric_metadata::<f64>(db, "last_modularity")? {
        if !q.is_finite() {
            return Err(astria_core::AstriaError::Graph(
                "Non-finite last_modularity".into(),
            ));
        }
        report.push_str(&format!(" | **Modularity:** {q:.3}"));
    }
    report.push_str("\n\n");
    if let Some(version) = metadata(db, "pipeline_version")? {
        report.push_str(&format!("_Built by astria v{}._\n\n", escape(&version)));
    }
    report.push_str("Start with Production Code for implementation and change impact. Documentation and Tests, Benchmarks and Examples provide separate context. File ranks count connected, located nodes; unresolved stubs and references are excluded.\n\n");

    // Every class gets its own file/hub budget: a large test suite or docs
    // corpus cannot crowd production modules out of the orientation report.
    for category in ORIENTATIONS {
        report.push_str(&format!("## {}\n\n### Key Files\n\n", category.title()));
        let mut files: BTreeMap<&str, i64> = BTreeMap::new();
        let mut hubs: Vec<_> = sources
            .nodes
            .iter()
            .filter(|(_, n)| n.orientation == category)
            .collect();
        for (_, node) in &hubs {
            *files.entry(&node.file).or_default() += node.degree;
        }
        let mut files: Vec<_> = files
            .into_iter()
            .filter(|(_, degree)| *degree > 0)
            .collect();
        files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        if files.is_empty() {
            report.push_str("No connected files in this category.\n");
        }
        for (file, degree) in files.iter().take(10) {
            report.push_str(&format!(
                "- {} ({degree} edge endpoints)\n",
                sources.link(&sources.display(file), file, None)
            ));
        }
        report.push_str("\n### Hub Nodes (God Nodes)\n\n");
        hubs.sort_by(|(ia, a), (ib, b)| b.degree.cmp(&a.degree).then_with(|| ia.cmp(ib)));
        let hubs: Vec<_> = hubs
            .into_iter()
            .filter(|(_, n)| n.degree > 0)
            .take(10)
            .collect();
        if hubs.is_empty() {
            report.push_str("No connected hub nodes in this category.\n");
        }
        for (id, node) in hubs {
            let community = node
                .community
                .map(|c| labels.get(&c).cloned().unwrap_or_else(|| c.to_string()))
                .unwrap_or_else(|| "—".into());
            report.push_str(&format!(
                "- {} (degree: {}, community: {}) — {}\n",
                sources.node_link(id, &node.label),
                node.degree,
                escape(&community),
                escape(&sources.display(&node.file))
            ));
        }
        report.push('\n');
    }

    report.push_str("## Communities\n\n");
    let mut stmt = db.prepare("SELECT id, label, size, cohesion, summary, label_source FROM communities ORDER BY size DESC, id ASC")?;
    #[allow(clippy::type_complexity)]
    let communities: Vec<(i64, String, i64, Option<f64>, Option<String>, String)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    if communities.is_empty() {
        report.push_str("No communities detected.\n\n");
    }
    // Group by the dominant source class, with production first on ties.
    let mut membership: HashMap<i64, [usize; 3]> = HashMap::new();
    for node in sources.nodes.values() {
        if let Some(c) = node.community {
            let index = node.orientation.index();
            membership.entry(c).or_default()[index] += 1;
        }
    }
    for (category_index, category) in ORIENTATIONS.iter().enumerate() {
        let selected: Vec<_> = communities
            .iter()
            .filter(|(id, ..)| {
                membership
                    .get(id)
                    .map(|counts| {
                        (0..3).max_by_key(|&i| (counts[i], std::cmp::Reverse(i)))
                            == Some(category_index)
                    })
                    .unwrap_or(false)
            })
            .collect();
        if selected.is_empty() {
            continue;
        }
        report.push_str(&format!("### {}\n\n", category.title()));
        for (id, label, size, cohesion, summary, source) in selected.iter().take(10).copied() {
            let coh = cohesion
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".into());
            let mut files: Vec<_> = sources
                .nodes
                .values()
                .filter(|n| n.community == Some(*id))
                .map(|n| n.file.as_str())
                .collect();
            files.sort_unstable();
            files.dedup();
            let loci = files
                .iter()
                .take(3)
                .map(|file| sources.link(&sources.display(file), file, None))
                .collect::<Vec<_>>()
                .join(", ");
            report.push_str(&format!(
                "- **{}** `[{}]` ({} nodes, cohesion {}) — {}\n",
                escape(label),
                escape(source),
                size,
                coh,
                loci
            ));
            if let Some(summary) = summary.as_ref().filter(|s| !s.is_empty()) {
                report.push_str(&format!("  {}\n", escape(summary)));
            }
        }
        if selected.len() > 10 {
            report.push_str(&format!(
                "\n... and {} more in this category.\n",
                selected.len() - 10
            ));
        }
        report.push('\n');
    }
    let unlocated: Vec<_> = communities
        .iter()
        .filter(|(id, ..)| !membership.contains_key(id))
        .collect();
    if !unlocated.is_empty() {
        report.push_str("### Communities without Source Loci\n\n");
        for (_, label, size, _, _, source) in unlocated.iter().take(10).copied() {
            report.push_str(&format!(
                "- **{}** `[{}]` ({} nodes)\n",
                escape(label),
                escape(source),
                size
            ));
        }
        report.push('\n');
    }

    report.push_str("## Surprising Connections\n\nRanked by novelty (smaller community size divided by the number of bridges). Novelty is distinct from relationship confidence: EXTRACTED records syntax, RESOLVED records a name binding, and INFERRED records a heuristic or semantic hypothesis. A missing confidence score remains unknown. Each source category shows up to 25 distinct relationships.\n\n");
    let mut evidence_stmt = db.prepare("SELECT confidence, confidence_score, source_file FROM edges WHERE source = ?1 AND target = ?2 AND relation = ?3 ORDER BY confidence, confidence_score DESC, source_file, id")?;
    let mut seen = HashSet::new();
    let mut relationships: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut relationship_counts: BTreeMap<usize, usize> = BTreeMap::new();
    let ranked_relationships = astria_analyze::ranked_surprising_connections(db)?;
    for edge in &ranked_relationships {
        if !seen.insert((&edge.source, &edge.target, &edge.relation)) {
            continue;
        }
        let category = sources
            .nodes
            .get(&edge.source)
            .map(|n| n.orientation)
            .or_else(|| sources.nodes.get(&edge.target).map(|n| n.orientation));
        let index = category
            .and_then(|c| ORIENTATIONS.iter().position(|o| *o == c))
            .unwrap_or(3);
        let count = relationship_counts.entry(index).or_default();
        *count += 1;
        if *count > MAX_RELATIONSHIPS_PER_CATEGORY {
            continue;
        }
        let rows: Vec<(String, Option<f64>, String)> = evidence_stmt
            .query_map(
                rusqlite::params![edge.source, edge.target, edge.relation],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?
            .collect::<rusqlite::Result<_>>()?;
        let evidence = rows
            .iter()
            .map(|(tier, score, file)| {
                let score = score
                    .map(|s| format!("{s:.2}"))
                    .unwrap_or_else(|| "unknown".into());
                let locus = if file.is_empty() {
                    "no source locus".into()
                } else {
                    sources.link(&sources.display(file), file, None)
                };
                format!("{} {score} at {locus}", escape(tier))
            })
            .collect::<Vec<_>>()
            .join("; ");
        let evidence = if evidence.is_empty() {
            "no persisted edge evidence".into()
        } else {
            evidence
        };
        relationships.entry(index).or_default().push(format!(
            "- {} -> {} ({}) [{} -> {}] (novelty: {:.2}; evidence: {})\n",
            sources.node_link(&edge.source, &edge.source_label),
            sources.node_link(&edge.target, &edge.target_label),
            escape(&edge.relation),
            escape(&label_of(edge.source_community)),
            escape(&label_of(edge.target_community)),
            edge.score,
            evidence
        ));
    }
    if relationships.is_empty() {
        report.push_str("No cross-community connections found.\n\n");
    }
    for (index, rows) in relationships {
        let title = ORIENTATIONS
            .get(index)
            .map(|c| c.title())
            .unwrap_or("Unlocated Relationships");
        report.push_str(&format!("### {title}\n\n"));
        for row in rows {
            report.push_str(&row);
        }
        let omitted = relationship_counts[&index].saturating_sub(MAX_RELATIONSHIPS_PER_CATEGORY);
        if omitted > 0 {
            report.push_str(&format!(
                "\n... and {omitted} more relationships in this category.\n"
            ));
        }
        report.push('\n');
    }

    let hyperedges = astria_build::hyperedges::load_all(db)?;
    if !hyperedges.is_empty() {
        report.push_str(&format!(
            "## Hyperedges ({} group relationships)\n\n",
            hyperedges.len()
        ));
        for h in hyperedges.iter().take(15) {
            let score = h
                .score
                .map(|s| format!("{s:.2}"))
                .unwrap_or_else(|| "unknown".into());
            report.push_str(&format!(
                "- **{}** — {} members [{} {}] `id: {}`\n",
                escape(&h.label),
                h.nodes.len(),
                h.confidence,
                score,
                escape(&h.id)
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
    if let Some(merged) = numeric_metadata::<i64>(db, "last_dedup_merged")?.filter(|c| *c > 0) {
        report.push_str(&format!("## Merged Duplicates\n\n{merged} near-duplicate node(s) merged into canonical entities.\n\n"));
    }
    report.push_str("## Suggested Questions\n\n");
    for q in &analysis.suggested_questions {
        report.push_str(&format!("- {}\n", escape(q)));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_analyze::{NodeAnalysis, SurprisingEdge};
    use astria_core::db::open_db_in_memory;

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
        assert!(report.contains("[Alpha](../f.py)"));
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
