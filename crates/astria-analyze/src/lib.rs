// astria-analyze: god nodes, surprises, question detection, and impact analysis

pub mod affected;

use rusqlite::Connection;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct NodeAnalysis {
    pub id: String,
    pub label: String,
    pub degree: usize,
    pub community: Option<u32>,
    /// Bare-name call stub (unresolved call target).
    pub is_stub: bool,
}

#[derive(Debug, Clone)]
pub struct SurprisingEdge {
    pub source: String,
    pub source_label: String,
    pub target: String,
    pub target_label: String,
    pub relation: String,
    pub source_community: Option<u32>,
    pub target_community: Option<u32>,
    /// Novelty score: larger, more cohesive communities joined by fewer
    /// edges score higher. Higher = more surprising.
    pub score: f64,
}

#[derive(Debug)]
pub struct AnalysisResult {
    pub god_nodes: Vec<NodeAnalysis>,
    pub surprising_connections: Vec<SurprisingEdge>,
    pub suggested_questions: Vec<String>,
}

pub fn analyze(db: &Connection) -> astria_core::Result<AnalysisResult> {
    let god_nodes = compute_god_nodes(db)?;
    let surprising = compute_surprising_connections(db)?;
    let questions = suggest_questions(db, &god_nodes, &surprising)?;
    Ok(AnalysisResult {
        god_nodes,
        surprising_connections: surprising,
        suggested_questions: questions,
    })
}

/// How many cross-community edges the report shows, ranked by novelty.
const MAX_SURPRISING: usize = 25;
/// Communities smaller than this are ignored when scoring surprising
/// connections — a singleton "community" reaching out is not surprising.
const MIN_COMMUNITY_SIZE: usize = 2;

/// Degree of every node, merged from two index-friendly group-bys.
fn all_degrees(db: &Connection) -> astria_core::Result<HashMap<String, usize>> {
    let mut degrees: HashMap<String, usize> = HashMap::new();
    {
        let mut stmt = db.prepare("SELECT source, COUNT(*) FROM edges GROUP BY source")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?;
        for (id, count) in rows.flatten() {
            *degrees.entry(id).or_insert(0) += count;
        }
    }
    {
        let mut stmt = db.prepare("SELECT target, COUNT(*) FROM edges GROUP BY target")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?;
        for (id, count) in rows.flatten() {
            *degrees.entry(id).or_insert(0) += count;
        }
    }
    Ok(degrees)
}

fn compute_god_nodes(db: &Connection) -> astria_core::Result<Vec<NodeAnalysis>> {
    let degrees = all_degrees(db)?;

    let mut nodes: Vec<NodeAnalysis> = Vec::new();
    {
        let mut stmt = db.prepare("SELECT id, label, community, file_type FROM nodes")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for (id, label, community, file_type) in rows.flatten() {
            nodes.push(NodeAnalysis {
                degree: degrees.get(&id).copied().unwrap_or(0),
                community: community.map(|c| c as u32),
                id,
                label,
                is_stub: file_type == "stub",
            });
        }
    }

    // Degree centrality normalized by the max degree, written for every node.
    let max_degree = nodes.iter().map(|n| n.degree).max().unwrap_or(0);
    if max_degree > 0 {
        let tx = db.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE nodes SET degree_centrality = ?1 WHERE id = ?2")?;
            for node in &nodes {
                stmt.execute(rusqlite::params![
                    node.degree as f64 / max_degree as f64,
                    node.id
                ])?;
            }
        }
        tx.commit()?;
    }

    // Bare-name call stubs ("get", "join") accumulate every unresolvable
    // call edge, so they dominate naive hub lists. Rank real symbols first;
    // fall back to stubs only when the graph has nothing else.
    nodes.sort_by(|a, b| b.degree.cmp(&a.degree).then_with(|| a.id.cmp(&b.id)));
    let mut top10: Vec<NodeAnalysis> = Vec::new();
    for node in nodes.iter().filter(|n| !n.is_stub) {
        if top10.len() == 10 {
            break;
        }
        top10.push(node.clone());
    }
    if top10.is_empty() {
        top10 = nodes.into_iter().take(10).collect();
    }
    Ok(top10)
}

/// Cross-community edges ranked by novelty: bigger communities joined by
/// fewer edges score higher — two large cohesive communities sharing a
/// single edge stand out, while a swarm of edges between two communities
/// is just an interface. Results are capped to MAX_SURPRISING.
fn compute_surprising_connections(db: &Connection) -> astria_core::Result<Vec<SurprisingEdge>> {
    // Community sizes come from the node assignments themselves — the
    // authoritative source, valid even when the communities table has not
    // been populated.
    let community_sizes: HashMap<i64, i64> = {
        let mut stmt = db.prepare(
            "SELECT community, COUNT(*) FROM nodes WHERE community IS NOT NULL GROUP BY community",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let mut stmt = db.prepare(
        "SELECT e.source, s.label, e.target, t.label, e.relation, s.community, t.community
         FROM edges e
         JOIN nodes s ON s.id = e.source JOIN nodes t ON t.id = e.target
         WHERE s.community IS NOT NULL AND t.community IS NOT NULL AND s.community != t.community",
    )?;
    let edges: Vec<SurprisingEdge> = stmt
        .query_map([], |row| {
            Ok(SurprisingEdge {
                source: row.get(0)?,
                source_label: row.get(1)?,
                target: row.get(2)?,
                target_label: row.get(3)?,
                relation: row.get(4)?,
                source_community: row.get::<_, Option<i64>>(5)?.map(|c| c as u32),
                target_community: row.get::<_, Option<i64>>(6)?.map(|c| c as u32),
                score: 0.0,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    if edges.is_empty() {
        return Ok(edges);
    }

    // How many edges connect each community pair?
    let mut pair_counts: HashMap<(u32, u32), usize> = HashMap::new();
    for edge in &edges {
        let (ca, cb) = (
            edge.source_community.unwrap_or(0),
            edge.target_community.unwrap_or(0),
        );
        let (lo, hi) = (ca.min(cb), ca.max(cb));
        *pair_counts.entry((lo, hi)).or_insert(0) += 1;
    }

    let mut scored: Vec<SurprisingEdge> = edges
        .into_iter()
        .filter(|e| {
            let (ca, cb) = (
                e.source_community.unwrap_or(0) as i64,
                e.target_community.unwrap_or(0) as i64,
            );
            let size_ok =
                |c: i64| community_sizes.get(&c).copied().unwrap_or(0) >= MIN_COMMUNITY_SIZE as i64;
            size_ok(ca) && size_ok(cb)
        })
        .map(|mut e| {
            let (ca, cb) = (
                e.source_community.unwrap_or(0),
                e.target_community.unwrap_or(0),
            );
            let size_a = community_sizes.get(&(ca as i64)).copied().unwrap_or(0) as f64;
            let size_b = community_sizes.get(&(cb as i64)).copied().unwrap_or(0) as f64;
            let (lo, hi) = (ca.min(cb), ca.max(cb));
            let between = pair_counts.get(&(lo, hi)).copied().unwrap_or(1) as f64;
            e.score = size_a.min(size_b) / between;
            e
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.target.cmp(&b.target))
    });
    scored.truncate(MAX_SURPRISING);
    Ok(scored)
}

/// Questions grounded in actual graph signals instead of one repeated
/// template: the top hub, the strongest cross-community bridge, and orphan
/// files that nothing references. Every question names its evidence so an
/// agent can act on it directly.
fn suggest_questions(
    db: &Connection,
    god_nodes: &[NodeAnalysis],
    surprising: &[SurprisingEdge],
) -> astria_core::Result<Vec<String>> {
    let mut questions = Vec::new();

    if let Some(top) = god_nodes.first() {
        questions.push(format!(
            "Why does {} have {} connections — shared core or coupling problem?",
            top.label, top.degree
        ));
    }

    if let Some(bridge) = surprising.first() {
        questions.push(format!(
            "{} {} {} crosses a community boundary - intentional or accidental coupling?",
            bridge.source_label, bridge.relation, bridge.target_label
        ));
    }

    // Orphan files: files that have nodes but no node participates in any
    // edge — dead code, dynamically-loaded code, or extraction gaps.
    let mut orphan_files: Vec<String> = Vec::new();
    {
        let mut all_files = db.prepare("SELECT DISTINCT source_file FROM nodes")?;
        let mut with_edges = db.prepare(
            "SELECT DISTINCT n.source_file FROM nodes n JOIN edges e ON e.source = n.id \
             UNION \
             SELECT DISTINCT n.source_file FROM nodes n JOIN edges e ON e.target = n.id",
        )?;
        let files: Vec<String> = all_files.query_map([], |r| r.get(0))?.flatten().collect();
        let connected: std::collections::HashSet<String> =
            with_edges.query_map([], |r| r.get(0))?.flatten().collect();
        for file in files {
            if !connected.contains(&file) {
                orphan_files.push(file);
            }
        }
    }
    orphan_files.sort();
    orphan_files.truncate(2);
    // Show paths relative to the project root so questions stay readable.
    let root = db.path().and_then(|db_path| {
        std::path::Path::new(db_path)
            .parent()?
            .parent()
            .map(|r| r.to_string_lossy().replace('\\', "/"))
    });
    for file in orphan_files {
        let display = match &root {
            Some(r) => astria_paths::relative_display(&file, r),
            None => file.clone(),
        };
        questions.push(format!(
            "Is {display} still used? Nothing in the graph references it."
        ));
    }

    let community_count: i64 = db
        .query_row(
            "SELECT COUNT(DISTINCT community) FROM nodes WHERE community IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if community_count > 1 {
        questions.push(format!(
            "What are the responsibilities of the {} communities?",
            community_count
        ));
    }
    Ok(questions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    fn seed_analyzed_graph(db: &Connection) {
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file, community) VALUES ('a', 'Alpha', 'code', 'f.py', 0);
            INSERT INTO nodes (id, label, file_type, source_file, community) VALUES ('b', 'Beta', 'code', 'f.py', 0);
            INSERT INTO nodes (id, label, file_type, source_file, community) VALUES ('c', 'Gamma', 'code', 'f.py', 1);
            INSERT INTO nodes (id, label, file_type, source_file, community) VALUES ('d', 'Delta', 'code', 'f.py', 1);
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('a', 'b', 'calls', 'EXTRACTED', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('a', 'c', 'calls', 'EXTRACTED', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('b', 'c', 'calls', 'EXTRACTED', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('c', 'd', 'calls', 'EXTRACTED', 'f.py');
        ",
        )
        .unwrap();
    }

    #[test]
    fn analyze_finds_god_nodes() {
        let db = open_db_in_memory().unwrap();
        seed_analyzed_graph(&db);
        let result = analyze(&db).unwrap();
        assert!(!result.god_nodes.is_empty());
        // Gamma has the highest in+out degree (2 in + 1 out)
        assert_eq!(result.god_nodes[0].id, "c");
    }

    #[test]
    fn analyze_finds_surprising_connections() {
        let db = open_db_in_memory().unwrap();
        seed_analyzed_graph(&db);
        let result = analyze(&db).unwrap();
        assert!(!result.surprising_connections.is_empty());
    }

    #[test]
    fn analyze_suggests_questions() {
        let db = open_db_in_memory().unwrap();
        seed_analyzed_graph(&db);
        let result = analyze(&db).unwrap();
        assert!(!result.suggested_questions.is_empty());
    }

    #[test]
    fn degree_centrality_written_for_all_nodes() {
        let db = open_db_in_memory().unwrap();
        seed_analyzed_graph(&db);
        analyze(&db).unwrap();
        let all: Vec<Option<f64>> = db
            .prepare("SELECT degree_centrality FROM nodes ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(all.len(), 4);
        assert!(
            all.iter().all(|d| d.is_some()),
            "every node gets a centrality"
        );
        let top = all.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();
        assert!(
            (top.unwrap() - 1.0).abs() < 1e-9,
            "max degree normalizes to 1.0"
        );
    }

    #[test]
    fn stubs_ranked_below_real_symbols() {
        let db = open_db_in_memory().unwrap();
        // 'get' stub has the highest raw degree but must not top the list
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, community) VALUES
                ('get', 'get', 'stub', '', NULL),
                ('real', 'RealService', 'code', 'src/real.rs', 0),
                ('caller', 'Caller', 'code', 'src/caller.rs', 0);
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('caller', 'get', 'calls', 'EXTRACTED', 'src/caller.rs'),
                ('caller', 'real', 'calls', 'EXTRACTED', 'src/caller.rs'),
                ('real', 'get', 'calls', 'EXTRACTED', 'src/real.rs');",
        )
        .unwrap();
        let result = analyze(&db).unwrap();
        assert_ne!(
            result.god_nodes[0].id, "get",
            "stub must not be the top god node"
        );
        assert!(result.god_nodes.iter().all(|n| n.id != "get"));
    }

    #[test]
    fn surprising_connections_ranked_and_capped() {
        let db = open_db_in_memory().unwrap();
        // Community A (6 nodes) and B (2 nodes) share one edge; A and C
        // (2 nodes) share 6 edges. The single rare bridge must score higher
        // than any edge of the busy pair.
        let mut sql = String::new();
        for i in 1..=6 {
            sql.push_str(&format!(
                "INSERT INTO nodes (id, label, file_type, source_file, community) VALUES ('a{i}','A{i}','code','f.py',0);\n"
            ));
        }
        sql.push_str(
            "INSERT INTO nodes (id, label, file_type, source_file, community) VALUES
                ('b1','B1','code','f.py',1),('b2','B2','code','f.py',1),
                ('c1','C1','code','f.py',2),('c2','C2','code','f.py',2);",
        );
        sql.push_str("INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('b1','a1','calls','EXTRACTED','f.py');\n");
        for i in 1..=6 {
            sql.push_str(&format!(
                "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('c1','a{i}','calls','EXTRACTED','f.py');\n"
            ));
        }
        db.execute_batch(&sql).unwrap();
        let result = analyze(&db).unwrap();
        assert!(!result.surprising_connections.is_empty());
        let top = &result.surprising_connections[0];
        assert_eq!(
            (top.source.as_str(), top.target.as_str()),
            ("b1", "a1"),
            "rare bridge between sizeable communities ranks first"
        );
        assert!(top.score > 0.0);
        // Scores are non-increasing
        for w in result.surprising_connections.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
    }

    #[test]
    fn singleton_communities_excluded_from_surprises() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, community) VALUES
                ('solo', 'Solo', 'code', 'f.py', 0),
                ('other', 'Other', 'code', 'f.py', 1);
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('solo', 'other', 'calls', 'EXTRACTED', 'f.py');",
        )
        .unwrap();
        // No communities table rows → sizes unknown → treated as too small
        let result = analyze(&db).unwrap();
        assert!(result.surprising_connections.is_empty());
    }

    #[test]
    fn suggested_questions_flag_orphan_files() {
        let db = open_db_in_memory().unwrap();
        seed_analyzed_graph(&db);
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('lonely', 'Lonely', 'code', 'orphan.rs');",
        )
        .unwrap();
        let result = analyze(&db).unwrap();
        assert!(
            result
                .suggested_questions
                .iter()
                .any(|q| q.contains("orphan.rs")),
            "orphan file question missing: {:?}",
            result.suggested_questions
        );
        // Questions must be distinct - no repeated template filler.
        let unique: std::collections::HashSet<&String> =
            result.suggested_questions.iter().collect();
        assert_eq!(unique.len(), result.suggested_questions.len());
    }
}
