// hyperedges: N-ary group relationships with a deterministic, fully local
// producer — unlike the official, whose hyperedges come from an optional LLM
// semantic pass. Two producers:
//   1. community::<id>    — each community of >= 3 real nodes, members are the
//                           top-degree nodes (relation `participate_in`)
//   2. hyper_ref::<name>  — reference nodes cited from >= 3 distinct files
//                           (relation `shares_reference`)
// Regenerated wholesale on every build (DELETE + INSERT), so re-runs are
// idempotent and stale groups never linger.

use graphify_core::Result;
use rusqlite::Connection;

/// Member cap per hyperedge — keeps graph.json and HTML hulls readable.
const MAX_MEMBERS: usize = 12;
const MIN_MEMBERS: usize = 3;

pub fn generate(db: &Connection) -> Result<usize> {
    let mut hyperedges: Vec<HyperEdgeRow> = Vec::new();

    // 1. Communities (requires clustering to have run).
    let comm_count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM communities WHERE size >= ?1",
            rusqlite::params![MIN_MEMBERS as i64],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if comm_count > 0 {
        let mut stmt = db.prepare(
            "SELECT c.id, c.label FROM communities c WHERE c.size >= ?1 ORDER BY c.size DESC",
        )?;
        let comms: Vec<(i64, String)> = stmt
            .query_map([MIN_MEMBERS as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        let mut member_stmt = db.prepare(
            "SELECT id FROM nodes
             WHERE community = ?1 AND file_type NOT IN ('stub', 'rationale', 'reference')
             ORDER BY degree_centrality DESC, id
             LIMIT ?2",
        )?;
        for (cid, label) in &comms {
            let members: Vec<String> = member_stmt
                .query_map(rusqlite::params![cid, MAX_MEMBERS as i64], |r| r.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            if members.len() < MIN_MEMBERS {
                continue;
            }
            hyperedges.push(HyperEdgeRow {
                id: format!("community::{cid}"),
                label: label.clone(),
                nodes: members,
                relation: "participate_in".into(),
                confidence: "INFERRED".into(),
                score: Some(0.9),
            });
        }
    }

    // 2. Shared references: a reference node (str::<literal>) cited from
    //    >= 3 distinct files.
    let mut stmt = db.prepare(
        "SELECT e.target, n.label, COUNT(DISTINCT e.source_file) AS files
         FROM edges e JOIN nodes n ON n.id = e.target
         WHERE e.relation = 'references' AND n.file_type = 'reference'
         GROUP BY e.target HAVING files >= ?1 ORDER BY files DESC LIMIT 200",
    )?;
    let ref_rows: Vec<(String, String, i64)> = stmt
        .query_map([MIN_MEMBERS as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut files_stmt =
        db.prepare("SELECT DISTINCT e.source_file FROM edges e WHERE e.target = ?1 LIMIT ?2")?;
    for (target, label, _files) in &ref_rows {
        let mut members: Vec<String> = vec![target.clone()];
        let sources: Vec<String> = files_stmt
            .query_map(rusqlite::params![target, (MAX_MEMBERS - 1) as i64], |r| {
                r.get(0)
            })?
            .filter_map(|r| r.ok())
            .collect();
        members.extend(sources);
        if members.len() < MIN_MEMBERS {
            continue;
        }
        hyperedges.push(HyperEdgeRow {
            // `str::<literal>` -> `hyper_ref::<literal>`
            id: target.replacen("str::", "hyper_ref::", 1),
            label: format!("{} (shared reference)", label),
            nodes: members,
            relation: "shares_reference".into(),
            confidence: "INFERRED".into(),
            score: Some(0.8),
        });
    }

    let tx = db.unchecked_transaction()?;
    tx.execute("DELETE FROM hyperedges", [])?;
    let mut stmt = tx.prepare(
        "INSERT INTO hyperedges (id, label, nodes, relation, confidence, confidence_score, source_file)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, '')",
    )?;
    for h in &hyperedges {
        let nodes_json = serde_json::to_string(&h.nodes)?;
        stmt.execute(rusqlite::params![
            h.id,
            graphify_core::sanitize_label(&h.label),
            nodes_json,
            h.relation,
            h.confidence,
            h.score,
        ])?;
    }
    drop(stmt);
    tx.commit()?;
    Ok(hyperedges.len())
}

struct HyperEdgeRow {
    id: String,
    label: String,
    nodes: Vec<String>,
    relation: String,
    confidence: String,
    score: Option<f64>,
}

/// Load all hyperedges with parsed member lists (export/report/wiki/html).
pub fn load_all(db: &Connection) -> Result<Vec<HyperEdge>> {
    let mut stmt = db.prepare(
        "SELECT id, label, nodes, relation, confidence, confidence_score FROM hyperedges",
    )?;
    let rows: Vec<HyperEdge> = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<f64>>(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .filter_map(|(id, label, nodes_json, relation, confidence, score)| {
            let nodes = serde_json::from_str::<Vec<String>>(&nodes_json).ok()?;
            Some(HyperEdge {
                id,
                label,
                nodes,
                relation,
                confidence,
                score,
            })
        })
        .collect();
    Ok(rows)
}

pub struct HyperEdge {
    pub id: String,
    pub label: String,
    pub nodes: Vec<String>,
    pub relation: String,
    pub confidence: String,
    pub score: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphify_core::db::open_db_in_memory;

    fn seed(db: &Connection) {
        db.execute_batch(
            r#"
            INSERT INTO nodes (id, label, file_type, source_file, community, degree_centrality) VALUES
              ('a', 'a()', 'code', 'a.py', 1, 0.9),
              ('b', 'b()', 'code', 'b.py', 1, 0.8),
              ('c', 'C', 'code', 'c.py', 1, 0.7),
              ('str::plane_url', 'PLANE_URL', 'reference', 'global', NULL, NULL);
            INSERT INTO communities (id, label, cohesion, size) VALUES (1, 'Core', 0.5, 3);
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('a', 'str::plane_url', 'references', 'EXTRACTED', 'a.py'),
              ('b', 'str::plane_url', 'references', 'EXTRACTED', 'b.py'),
              ('c', 'str::plane_url', 'references', 'EXTRACTED', 'c.py');
            "#,
        )
        .unwrap();
    }

    #[test]
    fn community_hyperedge_generated() {
        let db = open_db_in_memory().unwrap();
        seed(&db);
        let n = generate(&db).unwrap();
        assert_eq!(n, 2, "expected community + reference hyperedge");
        let all = load_all(&db).unwrap();
        let comm = all.iter().find(|h| h.id == "community::1").unwrap();
        assert_eq!(comm.relation, "participate_in");
        assert_eq!(comm.nodes.len(), 3);
        assert_eq!(comm.label, "Core");
    }

    #[test]
    fn shared_reference_hyperedge_generated() {
        let db = open_db_in_memory().unwrap();
        seed(&db);
        generate(&db).unwrap();
        let all = load_all(&db).unwrap();
        let r = all.iter().find(|h| h.id == "hyper_ref::plane_url").unwrap();
        assert_eq!(r.relation, "shares_reference");
        assert_eq!(r.nodes.len(), 4, "ref node + 3 citing files");
        assert!(r.nodes.contains(&"str::plane_url".to_string()));
    }

    #[test]
    fn regenerate_is_idempotent() {
        let db = open_db_in_memory().unwrap();
        seed(&db);
        generate(&db).unwrap();
        generate(&db).unwrap();
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM hyperedges", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "regeneration must not duplicate rows");
    }

    #[test]
    fn small_community_skipped() {
        let db = open_db_in_memory().unwrap();
        seed(&db);
        db.execute("DELETE FROM edges WHERE source = 'c' OR target = 'c'", [])
            .unwrap();
        db.execute("DELETE FROM nodes WHERE id = 'c'", []).unwrap();
        db.execute("UPDATE communities SET size = 2", []).unwrap();
        generate(&db).unwrap();
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM hyperedges", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "community with < 3 members must be skipped");
    }
}
