use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;

pub struct MergeResult {
    pub nodes_added: i64,
    pub edges_added: i64,
    pub communities: usize,
    pub report: String,
}

pub struct DiffResult {
    pub nodes_added: i64,
    pub nodes_removed: i64,
    pub edges_added: i64,
    pub edges_removed: i64,
    pub added_node_labels: Vec<String>,
    pub removed_node_labels: Vec<String>,
}

pub fn merge_graphs(
    root_a: &Path,
    root_b: &Path,
    out_root: &Path,
) -> astria_core::Result<MergeResult> {
    let db_a = crate::pipeline::load_graph_db(root_a)?;
    let db_b = crate::pipeline::load_graph_db(root_b)?;

    let out_astria = out_root.join(".astria");
    std::fs::create_dir_all(&out_astria)?;
    let db = astria_core::db::open_db(&out_astria.join("db.sqlite"))?;

    let mut node_ids: HashSet<String> = HashSet::new();
    let mut nodes_added = 0i64;

    insert_nodes_from(&db, &db_a, &mut node_ids, &mut nodes_added)?;
    insert_nodes_from_dedup(&db, &db_b, &mut node_ids, &mut nodes_added)?;

    let mut edges_added = 0i64;
    insert_edges_from(&db, &db_a, &mut edges_added)?;
    insert_edges_from(&db, &db_b, &mut edges_added)?;

    let cluster_result = astria_cluster::cluster(&db)?;
    let analysis = astria_analyze::analyze(&db)?;
    let report = astria_report::generate_report(&db, &analysis)?;

    std::fs::write(out_astria.join("graph_report.md"), &report)?;

    Ok(MergeResult {
        nodes_added,
        edges_added,
        communities: cluster_result.communities.len(),
        report,
    })
}

pub fn diff_graphs(root_a: &Path, root_b: &Path) -> astria_core::Result<DiffResult> {
    let db_a = crate::pipeline::load_graph_db(root_a)?;
    let db_b = crate::pipeline::load_graph_db(root_b)?;

    let ids_a: HashSet<String> = collect_ids(&db_a, "SELECT id FROM nodes")?;
    let ids_b: HashSet<String> = collect_ids(&db_b, "SELECT id FROM nodes")?;

    let added_ids: Vec<String> = ids_b.difference(&ids_a).cloned().collect();
    let removed_ids: Vec<String> = ids_a.difference(&ids_b).cloned().collect();

    let edges_a: HashSet<(String, String, String)> = collect_edges(&db_a)?;
    let edges_b: HashSet<(String, String, String)> = collect_edges(&db_b)?;

    let added_edges = edges_b.difference(&edges_a).count() as i64;
    let removed_edges = edges_a.difference(&edges_b).count() as i64;

    let added_labels = query_labels(&db_b, &added_ids)?;
    let removed_labels = query_labels(&db_a, &removed_ids)?;

    Ok(DiffResult {
        nodes_added: added_ids.len() as i64,
        nodes_removed: removed_ids.len() as i64,
        edges_added: added_edges,
        edges_removed: removed_edges,
        added_node_labels: added_labels,
        removed_node_labels: removed_labels,
    })
}

// -- internal helpers --

fn insert_nodes_from(
    db: &Connection,
    source: &Connection,
    node_ids: &mut HashSet<String>,
    count: &mut i64,
) -> astria_core::Result<()> {
    let mut stmt = source.prepare(
        "SELECT id, label, file_type, source_file, source_line, docstring, community FROM nodes",
    )?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        String,
        String,
        String,
        String,
        Option<i64>,
        Option<String>,
        Option<i64>,
    )> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    for (id, label, ft, sf, line, doc, comm) in rows {
        db.execute(
            "INSERT OR IGNORE INTO nodes (id, label, file_type, source_file, source_line, docstring, community) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, label, ft, sf, line, doc, comm],
        )?;
        node_ids.insert(id);
        *count += 1;
    }
    Ok(())
}

fn insert_nodes_from_dedup(
    db: &Connection,
    source: &Connection,
    node_ids: &mut HashSet<String>,
    count: &mut i64,
) -> astria_core::Result<()> {
    let mut stmt = source.prepare(
        "SELECT id, label, file_type, source_file, source_line, docstring, community FROM nodes",
    )?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        String,
        String,
        String,
        String,
        Option<i64>,
        Option<String>,
        Option<i64>,
    )> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    for (id, label, ft, sf, line, doc, comm) in rows {
        if node_ids.contains(&id) {
            continue;
        }
        db.execute(
            "INSERT OR IGNORE INTO nodes (id, label, file_type, source_file, source_line, docstring, community) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, label, ft, sf, line, doc, comm],
        )?;
        node_ids.insert(id);
        *count += 1;
    }
    Ok(())
}

fn insert_edges_from(
    db: &Connection,
    source: &Connection,
    count: &mut i64,
) -> astria_core::Result<()> {
    // Older graphs (pre schema v4) have no source_line column on edges.
    let has_line: bool = source
        .prepare("PRAGMA table_info(edges)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .any(|col| col == "source_line");
    let mut stmt = source.prepare(if has_line {
        "SELECT source, target, relation, confidence, confidence_score, source_file, source_line FROM edges"
    } else {
        "SELECT source, target, relation, confidence, confidence_score, source_file, NULL FROM edges"
    })?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        String,
        String,
        String,
        String,
        Option<f64>,
        String,
        Option<i64>,
    )> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    for (src, tgt, rel, conf, score, sf, line) in rows {
        db.execute(
            "INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file, source_line) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![src, tgt, rel, conf, score, sf, line],
        )?;
        *count += 1;
    }
    Ok(())
}

fn collect_ids(db: &Connection, query: &str) -> astria_core::Result<HashSet<String>> {
    let mut stmt = db.prepare(query)?;
    let rows: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows.into_iter().collect())
}

fn collect_edges(db: &Connection) -> astria_core::Result<HashSet<(String, String, String)>> {
    let mut stmt = db.prepare("SELECT source, target, relation FROM edges")?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows.into_iter().collect())
}

fn query_labels(db: &Connection, ids: &[String]) -> astria_core::Result<Vec<String>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
    let q = format!(
        "SELECT label FROM nodes WHERE id IN ({})",
        placeholders.join(",")
    );
    let params: Vec<&dyn rusqlite::types::ToSql> = ids
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();
    let mut stmt = db.prepare(&q)?;
    let rows: Vec<String> = stmt
        .query_map(params.as_slice(), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_db(db: &Connection, nodes: &[(&str, &str, &str)], edges: &[(&str, &str, &str)]) {
        for &(id, label, sf) in nodes {
            db.execute(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES (?1, ?2, 'code', ?3)",
                rusqlite::params![id, label, sf],
            )
            .unwrap();
        }
        for &(src, tgt, rel) in edges {
            db.execute(
                "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES (?1, ?2, ?3, 'EXTRACTED', 'test.py')",
                rusqlite::params![src, tgt, rel],
            ).unwrap();
        }
    }

    fn make_graph_dir(
        nodes: &[(&str, &str, &str)],
        edges: &[(&str, &str, &str)],
    ) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let gf = dir.path().join(".astria");
        std::fs::create_dir_all(&gf).unwrap();
        let db_path = gf.join("db.sqlite");
        let db = astria_core::db::open_db(&db_path).unwrap();
        seed_db(&db, nodes, edges);
        dir
    }

    #[test]
    fn merge_two_disjoint_graphs() {
        let dir_a = make_graph_dir(
            &[("a::Foo", "Foo", "a.py"), ("a::Bar", "Bar", "a.py")],
            &[("a::Foo", "a::Bar", "calls")],
        );
        let dir_b = make_graph_dir(
            &[("b::Baz", "Baz", "b.py"), ("b::Qux", "Qux", "b.py")],
            &[("b::Baz", "b::Qux", "calls")],
        );
        let out = tempfile::tempdir().unwrap();

        let result = merge_graphs(dir_a.path(), dir_b.path(), out.path()).unwrap();
        assert_eq!(result.nodes_added, 4);
        assert_eq!(result.edges_added, 2);
    }

    #[test]
    fn merge_overlapping_nodes_deduped() {
        let nodes_a = &[("shared::X", "X", "a.py")];
        let nodes_b = &[("shared::X", "X", "b.py")]; // same ID
        let dir_a = make_graph_dir(nodes_a, &[]);
        let dir_b = make_graph_dir(nodes_b, &[]);
        let out = tempfile::tempdir().unwrap();

        let result = merge_graphs(dir_a.path(), dir_b.path(), out.path()).unwrap();
        assert_eq!(result.nodes_added, 1, "overlapping node should be deduped");
    }

    #[test]
    fn diff_identical_graphs() {
        let nodes = &[("n1", "Alpha", "f.py"), ("n2", "Beta", "f.py")];
        let edges = &[("n1", "n2", "calls")];
        let dir_a = make_graph_dir(nodes, edges);
        let dir_b = make_graph_dir(nodes, edges);

        let result = diff_graphs(dir_a.path(), dir_b.path()).unwrap();
        assert_eq!(result.nodes_added, 0);
        assert_eq!(result.nodes_removed, 0);
        assert_eq!(result.edges_added, 0);
        assert_eq!(result.edges_removed, 0);
    }

    #[test]
    fn diff_graph_with_added_nodes() {
        let dir_a = make_graph_dir(&[("n1", "Alpha", "f.py")], &[]);
        let dir_b = make_graph_dir(&[("n1", "Alpha", "f.py"), ("n2", "Beta", "g.py")], &[]);

        let result = diff_graphs(dir_a.path(), dir_b.path()).unwrap();
        assert_eq!(result.nodes_added, 1);
        assert_eq!(result.nodes_removed, 0);
        assert!(result.added_node_labels.contains(&"Beta".to_string()));
    }

    #[test]
    fn diff_graph_with_removed_nodes() {
        let dir_a = make_graph_dir(&[("n1", "Alpha", "f.py"), ("n2", "Beta", "g.py")], &[]);
        let dir_b = make_graph_dir(&[("n1", "Alpha", "f.py")], &[]);

        let result = diff_graphs(dir_a.path(), dir_b.path()).unwrap();
        assert_eq!(result.nodes_added, 0);
        assert_eq!(result.nodes_removed, 1);
        assert!(result.removed_node_labels.contains(&"Beta".to_string()));
    }
}
