// postgres: introspect a live PostgreSQL schema (tables, views, foreign keys)
// into an Extraction under a virtual `pg::{db}` file node. Requires the `psql`
// CLI on PATH — we shell out instead of linking a client library so default
// builds stay dependency-free. Read-only: two SELECTs against the catalog.

use std::path::PathBuf;
use std::process::Command;

use graphify_core::ids::normalize_id;
use graphify_core::GraphifyError;
use graphify_core::Result;
use graphify_extract::{ExtractedEdge, ExtractedNode, Extraction};

/// Unit separator between psql output fields (psql -F).
const FIELD_SEP: char = '\u{1f}';

/// The catalog queries: (name, sql). Output rows are joined by FIELD_SEP.
pub const INTROSPECTION_SQL: &[(&str, &str)] = &[
    (
        "relations",
        // tables and views in the public schema
        "SELECT c.relname, CASE c.relkind WHEN 'r' THEN 'table' WHEN 'v' THEN 'view' ELSE 'other' END \
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'public' AND c.relkind IN ('r','v') ORDER BY c.relname;",
    ),
    (
        "foreign_keys",
        "SELECT tc.relname, cc.relname \
         FROM pg_constraint con \
         JOIN pg_class tc ON tc.oid = con.conrelid \
         JOIN pg_class cc ON cc.oid = con.confrelid \
         JOIN pg_namespace n ON n.oid = tc.relnamespace \
         WHERE n.nspname = 'public' AND con.contype = 'f';",
    ),
];

fn pg_node_id(db: &str, relation: &str) -> String {
    format!("pg_{}::{}", normalize_id(db), normalize_id(relation))
}

/// Build an Extraction from raw psql TSV output sections, in
/// INTROSPECTION_SQL order (relations, then foreign_keys).
pub fn build_extraction(db_name: &str, sections: &[&str]) -> Result<Extraction> {
    let mut nodes: Vec<ExtractedNode> = Vec::new();
    let mut edges: Vec<ExtractedEdge> = Vec::new();

    let file_id = format!("pg_{}", normalize_id(db_name));
    nodes.push(ExtractedNode {
        id: file_id.clone(),
        label: format!("{db_name} (postgres)"),
        source_file: PathBuf::from(format!("postgres://{db_name}")),
        source_line: None,
        docstring: Some("Live PostgreSQL schema introspection".to_string()),
        signature: None,
        node_type: "code".to_string(),
    });

    let mut relation_kinds: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    let Some(rel_rows) = sections.first() else {
        return Err(GraphifyError::Graph("missing relations section".into()));
    };
    for line in rel_rows.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split(FIELD_SEP);
        let (Some(name), Some(kind)) = (parts.next(), parts.next()) else {
            continue;
        };
        let id = pg_node_id(db_name, name);
        relation_kinds.insert(id.clone(), kind.to_string());
        let display = if kind == "view" {
            format!("{name} (view)")
        } else {
            name.to_string()
        };
        nodes.push(ExtractedNode {
            id,
            label: display,
            source_file: PathBuf::from(format!("postgres://{db_name}")),
            source_line: Some(nodes.len() as u32),
            docstring: None,
            signature: None,
            node_type: "code".to_string(),
        });
    }
    for (id, _) in relation_kinds.iter() {
        edges.push(ExtractedEdge {
            source: file_id.clone(),
            target: id.clone(),
            relation: "contains".to_string(),
            confidence: "EXTRACTED".to_string(),
            confidence_score: Some(1.0),
            source_file: PathBuf::from(format!("postgres://{db_name}")),
            source_line: Some(1),
        });
    }

    if let Some(fk_rows) = sections.get(1) {
        for line in fk_rows.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let mut parts = line.split(FIELD_SEP);
            let (Some(from), Some(to)) = (parts.next(), parts.next()) else {
                continue;
            };
            edges.push(ExtractedEdge {
                source: pg_node_id(db_name, from.trim()),
                target: pg_node_id(db_name, to.trim()),
                relation: "references".to_string(),
                confidence: "EXTRACTED".to_string(),
                confidence_score: Some(1.0),
                source_file: PathBuf::from(format!("postgres://{db_name}")),
                source_line: Some(1),
            });
        }
    }

    Ok(Extraction {
        file_path: PathBuf::from(format!("postgres://{db_name}")),
        language: "SQL".to_string(),
        nodes,
        edges,
    })
}

/// Extract the database name from a DSN (postgres://user:pass@host:port/db)
/// or a bare keyword string. Never exposes credentials — only the db name is
/// used for ids.
pub fn dsn_db_name(dsn: &str) -> String {
    // Keyword form: `host=localhost dbname=other`
    if let Some(rest) = dsn
        .split_whitespace()
        .find_map(|kv| kv.strip_prefix("dbname="))
    {
        return rest.to_string();
    }
    let path = dsn
        .split("://")
        .last()
        .unwrap_or(dsn)
        .split('?')
        .next()
        .unwrap_or("");
    let db = path
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("postgres");
    db.to_string()
}

/// Run the introspection through `psql` and produce an Extraction. Fails
/// loudly if psql is missing or the DSN is unreachable.
pub fn ingest_postgres(dsn: &str) -> Result<Extraction> {
    let db_name = dsn_db_name(dsn);
    let mut sections: Vec<String> = Vec::new();
    for (_, sql) in INTROSPECTION_SQL {
        let out = Command::new("psql")
            .arg(dsn)
            .args(["-A", "-t", "-F", &FIELD_SEP.to_string(), "-c", sql])
            .output()
            .map_err(|e| {
                GraphifyError::Graph(format!("failed to run psql (is it on PATH?): {e}"))
            })?;
        if !out.status.success() {
            return Err(GraphifyError::Graph(format!(
                "psql introspection failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        sections.push(String::from_utf8_lossy(&out.stdout).to_string());
    }
    let refs: Vec<&str> = sections.iter().map(|s| s.as_str()).collect();
    build_extraction(&db_name, &refs)
}

/// Path of the virtual source file used for a postgres extraction — lets the
/// napi layer key cache/prune behavior.
pub fn virtual_file_for(db_name: &str) -> PathBuf {
    PathBuf::from(format!("postgres://{db_name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_nodes_and_fk_edges_from_psql_output() {
        let relations = "users\u{1f}table\norders\u{1f}table\naudit_log\u{1f}view\n";
        let fks = "orders\u{1f}users\n";
        let ext = build_extraction("shop", &[relations, fks]).unwrap();
        assert!(ext
            .nodes
            .iter()
            .any(|n| n.id == "pg_shop" && n.label == "shop (postgres)"));
        assert!(ext.nodes.iter().any(|n| n.id == "pg_shop::users"));
        assert!(ext.nodes.iter().any(|n| n.label == "audit_log (view)"));
        assert!(ext
            .edges
            .iter()
            .any(|e| e.relation == "contains" && e.target == "pg_shop::users"));
        assert!(ext.edges.iter().any(|e| e.relation == "references"
            && e.source == "pg_shop::orders"
            && e.target == "pg_shop::users"));
    }

    #[test]
    fn dsn_db_name_extraction() {
        assert_eq!(dsn_db_name("postgres://u:p@host:5432/shop"), "shop");
        assert_eq!(
            dsn_db_name("postgres://u:p@host:5432/shop?sslmode=disable"),
            "shop"
        );
        assert_eq!(dsn_db_name("host=localhost dbname=other"), "other");
    }

    #[test]
    fn sql_only_reads_catalog() {
        // Guard against accidental writes in the introspection SQL.
        for (_, sql) in INTROSPECTION_SQL {
            let lower = sql.to_lowercase();
            assert!(!lower.contains("insert"), "{sql}");
            assert!(!lower.contains("update "), "{sql}");
            assert!(!lower.contains("delete"), "{sql}");
            assert!(!lower.contains("drop"), "{sql}");
            assert!(lower.contains("select"));
        }
    }
}
