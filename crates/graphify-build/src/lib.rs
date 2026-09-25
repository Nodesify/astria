// graphify-build: merge extractions into SQLite graph

pub mod dedup;
pub mod hyperedges;
pub mod minhash;
pub mod validate;

use graphify_core::Result;
use graphify_extract::Extraction;
use graphify_paths::normalize;
use rusqlite::{Connection, Transaction};

#[derive(Debug)]
pub struct BuildResult {
    pub nodes_added: usize,
    pub edges_added: usize,
    pub duplicates_merged: usize,
}

pub fn build(extractions: &[Extraction], db: &Connection) -> Result<BuildResult> {
    // Fail loud on corrupted extractions before anything touches the DB.
    validate::assert_valid(extractions)?;

    let mut nodes_added = 0;
    let mut edges_added = 0;
    let mut duplicates_merged = 0;

    let tx = db.unchecked_transaction()?;

    for extraction in extractions {
        let file_path = normalize(&extraction.file_path);

        // Delete old edges first (foreign key references nodes), then old nodes.
        // Must also delete edges from other files that reference nodes being removed.
        tx.execute(
            "DELETE FROM edges WHERE source_file = ?1",
            rusqlite::params![file_path],
        )?;
        tx.execute(
            "DELETE FROM edges WHERE source IN (SELECT id FROM nodes WHERE source_file = ?1) OR target IN (SELECT id FROM nodes WHERE source_file = ?1)",
            rusqlite::params![file_path],
        )?;
        tx.execute(
            "DELETE FROM nodes WHERE source_file = ?1",
            rusqlite::params![file_path],
        )?;

        for node in &extraction.nodes {
            // An existing non-stub node with this id is a cross-file merge
            // (e.g. shared concepts); skip it. A stub (created speculatively
            // by an earlier file's edges) must be replaced by the real
            // definition via the upsert below.
            let existing_ft: Option<String> = tx
                .query_row(
                    "SELECT file_type FROM nodes WHERE id = ?1",
                    rusqlite::params![node.id],
                    |row| row.get(0),
                )
                .ok();
            if let Some(ft) = existing_ft {
                if ft != "stub" {
                    duplicates_merged += 1;
                    continue;
                }
            }

            let file_type = match node.node_type.as_str() {
                "rationale" => "rationale",
                "concept" | "entity" | "pattern" | "module" | "reference" | "package"
                | "mcp_config" | "mcp_server" | "mcp_command" | "mcp_package" | "env_var" => {
                    node.node_type.as_str()
                }
                _ => {
                    if extraction.language == "markdown" {
                        "document"
                    } else {
                        "code"
                    }
                }
            };

            // Insert the node; a real definition always replaces a stub with
            // the same id (stubs are created speculatively by edges and may
            // land before the definition's own extraction is processed).
            tx.execute(
                "INSERT INTO nodes (id, label, file_type, source_file, source_line, docstring, signature) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                   label = excluded.label,
                   file_type = excluded.file_type,
                   source_file = excluded.source_file,
                   source_line = excluded.source_line,
                   docstring = excluded.docstring,
                   signature = excluded.signature
                 WHERE nodes.file_type = 'stub'",
                rusqlite::params![
                    node.id,
                    graphify_core::sanitize_label(&node.label),
                    file_type,
                    normalize(&node.source_file),
                    node.source_line,
                    node.docstring.as_deref().map(graphify_core::sanitize_docstring),
                    node
                        .signature
                        .as_deref()
                        .map(graphify_core::sanitize_docstring),
                ],
            )?;
            nodes_added += 1;
        }

        for edge in &extraction.edges {
            // Ensure source node exists (stub if missing)
            ensure_node_exists(
                &tx,
                &edge.source,
                &edge.source,
                &normalize(&edge.source_file),
            )?;
            // Ensure target node exists (stub if missing)
            ensure_node_exists(
                &tx,
                &edge.target,
                &edge.target,
                &normalize(&edge.source_file),
            )?;

            tx.execute(
                "INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file, source_line) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    edge.source,
                    edge.target,
                    edge.relation,
                    edge.confidence,
                    edge.confidence_score,
                    normalize(&edge.source_file),
                    edge.source_line,
                ],
            )?;
            edges_added += 1;
        }
    }

    tx.commit()?;
    Ok(BuildResult {
        nodes_added,
        edges_added,
        duplicates_merged,
    })
}

/// Ensure a node with the given id exists. If not, insert a stub node.
fn ensure_node_exists(tx: &Transaction, id: &str, label: &str, source_file: &str) -> Result<()> {
    let exists: bool = tx
        .query_row(
            "SELECT COUNT(*) FROM nodes WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if !exists {
        tx.execute(
            "INSERT OR IGNORE INTO nodes (id, label, file_type, source_file) VALUES (?1, ?2, 'stub', ?3)",
            rusqlite::params![id, label, source_file],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphify_core::db::open_db_in_memory;
    use graphify_extract::{ExtractedEdge, ExtractedNode, Extraction};
    use std::path::PathBuf;

    fn make_extraction_at(
        path: &str,
        nodes: Vec<(&str, &str)>,
        edges: Vec<(&str, &str, &str)>,
    ) -> Extraction {
        Extraction {
            file_path: PathBuf::from(path),
            language: "Python".into(),
            nodes: nodes
                .into_iter()
                .map(|(id, label)| ExtractedNode {
                    id: id.into(),
                    label: label.into(),
                    source_file: PathBuf::from(path),
                    source_line: Some(1),
                    docstring: None,
                    signature: None,
                    node_type: "function".into(),
                })
                .collect(),
            edges: edges
                .into_iter()
                .map(|(src, tgt, rel)| ExtractedEdge {
                    source: src.into(),
                    target: tgt.into(),
                    relation: rel.into(),
                    confidence: "EXTRACTED".into(),
                    confidence_score: Some(1.0),
                    source_file: PathBuf::from(path),
                    source_line: Some(1),
                })
                .collect(),
        }
    }

    fn make_extraction(nodes: Vec<(&str, &str)>, edges: Vec<(&str, &str, &str)>) -> Extraction {
        make_extraction_at("test.py", nodes, edges)
    }

    #[test]
    fn build_inserts_nodes_and_edges() {
        let db = open_db_in_memory().unwrap();
        let ext = make_extraction(
            vec![("testpy::hello", "hello()"), ("testpy::world", "world()")],
            vec![("testpy::hello", "testpy::world", "calls")],
        );
        let result = build(&[ext], &db).unwrap();
        assert_eq!(result.nodes_added, 2);
        assert_eq!(result.edges_added, 1);
    }

    #[test]
    fn real_definition_replaces_stub() {
        // An edge in lib.rs can create a stub for a symbol defined in
        // pipeline.rs before pipeline.rs's extraction is processed — the real
        // definition must win, regardless of file order.
        let db = open_db_in_memory().unwrap();
        let caller = make_extraction_at(
            "lib.rs",
            vec![("srclib::caller", "caller()")],
            vec![("srclib::caller", "srcpipeline::helper", "calls")],
        );
        build(&[caller], &db).unwrap();

        let file_type: String = db
            .query_row(
                "SELECT file_type FROM nodes WHERE id = 'srcpipeline::helper'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(file_type, "stub");

        let definition = make_extraction_at(
            "pipeline.rs",
            vec![("srcpipeline::helper", "helper()")],
            vec![],
        );
        build(&[definition], &db).unwrap();

        let (file_type, label): (String, String) = db
            .query_row(
                "SELECT file_type, label FROM nodes WHERE id = 'srcpipeline::helper'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(file_type, "code");
        assert_eq!(label, "helper()");
    }

    #[test]
    fn build_replaces_file_on_rebuild() {
        let db = open_db_in_memory().unwrap();
        let ext1 = make_extraction(vec![("testpy::old", "old()")], vec![]);
        build(&[ext1], &db).unwrap();

        let ext2 = make_extraction(vec![("testpy::new", "new()")], vec![]);
        let result = build(&[ext2], &db).unwrap();
        assert_eq!(result.nodes_added, 1);

        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE id = 'testpy::old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn build_deduplicates_nodes_across_files() {
        let db = open_db_in_memory().unwrap();

        let ext1 = Extraction {
            file_path: PathBuf::from("a.py"),
            language: "Python".into(),
            nodes: vec![ExtractedNode {
                id: "shared::func".into(),
                label: "func()".into(),
                source_file: PathBuf::from("a.py"),
                source_line: Some(1),
                docstring: None,
                signature: None,
                node_type: "function".into(),
            }],
            edges: vec![],
        };
        let result1 = build(&[ext1], &db).unwrap();
        assert_eq!(result1.nodes_added, 1);
        assert_eq!(result1.duplicates_merged, 0);

        let ext2 = Extraction {
            file_path: PathBuf::from("b.py"),
            language: "Python".into(),
            nodes: vec![ExtractedNode {
                id: "shared::func".into(),
                label: "func()".into(),
                source_file: PathBuf::from("b.py"),
                source_line: Some(5),
                docstring: None,
                signature: None,
                node_type: "function".into(),
            }],
            edges: vec![],
        };
        let result2 = build(&[ext2], &db).unwrap();
        assert_eq!(result2.nodes_added, 0);
        assert_eq!(result2.duplicates_merged, 1);
    }

    #[test]
    fn build_handles_empty_extractions() {
        let db = open_db_in_memory().unwrap();
        let ext = Extraction {
            file_path: PathBuf::from("empty.py"),
            language: "Python".into(),
            nodes: vec![],
            edges: vec![],
        };
        let result = build(&[ext], &db).unwrap();
        assert_eq!(result.nodes_added, 0);
        assert_eq!(result.edges_added, 0);
        assert_eq!(result.duplicates_merged, 0);
    }

    #[test]
    fn build_multiple_extractions_in_one_call() {
        let db = open_db_in_memory().unwrap();

        let ext1 = Extraction {
            file_path: PathBuf::from("a.py"),
            language: "Python".into(),
            nodes: vec![ExtractedNode {
                id: "a::foo".into(),
                label: "foo()".into(),
                source_file: PathBuf::from("a.py"),
                source_line: Some(1),
                docstring: None,
                signature: None,
                node_type: "function".into(),
            }],
            edges: vec![],
        };

        let ext2 = Extraction {
            file_path: PathBuf::from("b.py"),
            language: "Python".into(),
            nodes: vec![ExtractedNode {
                id: "b::bar".into(),
                label: "bar()".into(),
                source_file: PathBuf::from("b.py"),
                source_line: Some(1),
                docstring: None,
                signature: None,
                node_type: "function".into(),
            }],
            edges: vec![ExtractedEdge {
                source: "b::bar".into(),
                target: "a::foo".into(),
                relation: "calls".into(),
                confidence: "EXTRACTED".into(),
                confidence_score: Some(1.0),
                source_file: PathBuf::from("b.py"),
                source_line: Some(2),
            }],
        };

        let result = build(&[ext1, ext2], &db).unwrap();
        assert_eq!(result.nodes_added, 2);
        assert_eq!(result.edges_added, 1);
    }
}
