// diagnose: read-only graph health report (port of upstream diagnostics.py).
// Dangling endpoints, self-loops, duplicate edges, stub inventory, and
// per-file-type counts. Never mutates the graph.

use std::collections::HashMap;

use rusqlite::Connection;

#[derive(Debug, Default)]
pub struct DiagnoseReport {
    pub node_count: usize,
    pub edge_count: usize,
    pub dangling_edges: usize,
    pub self_loops: usize,
    pub duplicate_edges: usize,
    pub stub_nodes: usize,
    pub unlinked_nodes: usize,
    pub file_type_counts: Vec<(String, usize)>,
    pub top_dangling_targets: Vec<(String, usize)>,
}

pub fn diagnose(db: &Connection) -> astria_core::Result<DiagnoseReport> {
    let mut report = DiagnoseReport::default();

    let mut stmt = db.prepare("SELECT COUNT(*) FROM nodes")?;
    report.node_count = stmt.query_row([], |r| r.get::<_, i64>(0))? as usize;

    let mut stmt = db.prepare("SELECT COUNT(*) FROM edges")?;
    report.edge_count = stmt.query_row([], |r| r.get::<_, i64>(0))? as usize;

    let mut stmt = db.prepare("SELECT COUNT(*) FROM nodes WHERE file_type = 'stub'")?;
    report.stub_nodes = stmt.query_row([], |r| r.get::<_, i64>(0))? as usize;

    // Dangling endpoints + self-loops in one pass.
    {
        let mut stmt = db.prepare(
            "SELECT e.source, e.target,
                    EXISTS(SELECT 1 FROM nodes n WHERE n.id = e.source),
                    EXISTS(SELECT 1 FROM nodes n WHERE n.id = e.target)
             FROM edges e",
        )?;
        let mut dangling_targets: HashMap<String, usize> = HashMap::new();
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let source: String = row.get(0)?;
            let target: String = row.get(1)?;
            let src_ok: bool = row.get(2)?;
            let tgt_ok: bool = row.get(3)?;
            if source == target {
                report.self_loops += 1;
            }
            if !src_ok || !tgt_ok {
                report.dangling_edges += 1;
                if !src_ok {
                    *dangling_targets.entry(source).or_insert(0) += 1;
                }
                if !tgt_ok {
                    *dangling_targets.entry(target).or_insert(0) += 1;
                }
            }
        }
        report.top_dangling_targets = {
            let mut v: Vec<(String, usize)> = dangling_targets.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            v.truncate(5);
            v
        };
    }

    // Duplicate edges: same (source, target, relation) more than once.
    {
        let mut stmt = db.prepare(
            "SELECT COUNT(*) FROM (
               SELECT source, target, relation FROM edges
               GROUP BY source, target, relation HAVING COUNT(*) > 1
             )",
        )?;
        report.duplicate_edges = stmt.query_row([], |r| r.get::<_, i64>(0))? as usize;
    }

    // Unlinked nodes (no edges at all) — usually stubs or orphan docs.
    {
        let mut stmt = db.prepare(
            "SELECT COUNT(*) FROM nodes n
             WHERE NOT EXISTS(SELECT 1 FROM edges e WHERE e.source = n.id OR e.target = n.id)",
        )?;
        report.unlinked_nodes = stmt.query_row([], |r| r.get::<_, i64>(0))? as usize;
    }

    // File-type inventory.
    {
        let mut stmt =
            db.prepare("SELECT file_type, COUNT(*) FROM nodes GROUP BY file_type ORDER BY 2 DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        report.file_type_counts = rows
            .filter_map(|r| r.ok())
            .map(|(ft, c)| (ft, c as usize))
            .collect();
    }

    Ok(report)
}

/// Human-readable rendering (CLI output).
pub fn render(report: &DiagnoseReport) -> String {
    let mut lines = Vec::new();
    lines.push("Graph diagnosis".to_string());
    lines.push("───────────────".to_string());
    lines.push(format!(
        "Nodes: {} | Edges: {} | Stubs: {} | Unlinked: {}",
        report.node_count, report.edge_count, report.stub_nodes, report.unlinked_nodes
    ));
    lines.push(format!(
        "Dangling edge endpoints: {} | Self-loops: {} | Duplicate edges: {}",
        report.dangling_edges, report.self_loops, report.duplicate_edges
    ));
    if !report.file_type_counts.is_empty() {
        let counts = report
            .file_type_counts
            .iter()
            .map(|(ft, c)| format!("{ft}: {c}"))
            .collect::<Vec<_>>()
            .join(" | ");
        lines.push(format!("Node types: {counts}"));
    }
    if !report.top_dangling_targets.is_empty() {
        lines.push("Top dangling targets (stub candidates):".to_string());
        for (target, count) in &report.top_dangling_targets {
            lines.push(format!("  {target} — {count} edge(s)"));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    #[test]
    fn reports_dangling_selfloops_and_duplicates() {
        let db = open_db_in_memory().unwrap();
        // Real builds cannot produce dangling edges (FK enforced) — simulate a
        // foreign-key-disabled legacy DB to exercise the defensive counters.
        db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        db.execute_batch(
            r#"
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('a', 'a()', 'code', 'f.py'),
              ('b', 'b()', 'code', 'f.py'),
              ('ghost', 'ghost', 'stub', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('a', 'b', 'calls', 'EXTRACTED', 'f.py'),
              ('a', 'b', 'calls', 'EXTRACTED', 'f.py'),
              ('a', 'missing', 'calls', 'INFERRED', 'f.py'),
              ('a', 'a', 'uses', 'INFERRED', 'f.py');
            "#,
        )
        .unwrap();
        let report = diagnose(&db).unwrap();
        assert_eq!(report.node_count, 3);
        assert_eq!(report.edge_count, 4);
        assert_eq!(report.duplicate_edges, 1);
        assert_eq!(report.dangling_edges, 1);
        assert_eq!(report.self_loops, 1);
        assert_eq!(
            report.top_dangling_targets,
            vec![("missing".to_string(), 1)]
        );
        assert_eq!(report.stub_nodes, 1);
        let text = render(&report);
        assert!(text.contains("Dangling edge endpoints: 1"));
        assert!(text.contains("missing"));
    }
}
