// Neo4j Cypher script export: writes an idempotent .cypher file (MERGE
// statements) that can be replayed with `cypher-shell -f` or pasted into
// the Neo4j Browser. Ported from upstream astria export.py neo4j logic,
// as a script generator instead of a live driver push.

use std::path::Path;

use rusqlite::Connection;

use astria_core::Result;

/// Uppercase relation names into valid Cypher relationship types
/// (CALLS, IMPORTS, ...), defaulting to RELATED_TO.
fn safe_rel(relation: &str) -> String {
    let mut out = String::new();
    for c in relation.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "RELATED_TO".to_string()
    } else {
        trimmed
    }
}

/// Escape a string for a double-quoted Cypher literal and strip control
/// characters that would corrupt the script file.
fn cypher_str(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Map a node's file_type to a Cypher label (Code, Document, Stub...).
fn node_label(file_type: &str) -> String {
    let mut chars = file_type.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Entity".to_string(),
    }
}

/// Write the graph as a replayable Cypher script. Returns the number of
/// statements written (nodes + edges).
pub fn export_cypher(db: &Connection, out_path: &Path) -> Result<usize> {
    let mut script = String::from(
        "// astria Neo4j import - idempotent, safe to re-run (MERGE)\n\
         // Import with: cypher-shell -u neo4j -p <password> -f astria.cypher\n",
    );

    let mut count = 0usize;
    {
        let mut stmt =
            db.prepare("SELECT id, label, file_type, source_file, community FROM nodes")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })?;
        for (id, label, file_type, source_file, community) in rows.flatten() {
            script.push_str(&format!(
                "MERGE (n:{} {{id: {}}}) SET n.label = {}, n.source_file = {}",
                node_label(&file_type),
                cypher_str(&id),
                cypher_str(&label),
                cypher_str(&source_file),
            ));
            if let Some(community) = community {
                script.push_str(&format!(", n.community = {community}"));
            }
            script.push_str(";\n");
            count += 1;
        }
    }

    {
        let mut stmt = db.prepare("SELECT source, target, relation, confidence FROM edges")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for (source, target, relation, confidence) in rows.flatten() {
            script.push_str(&format!(
                "MATCH (a {{id: {}}}), (b {{id: {}}}) MERGE (a)-[r:{}]->(b) SET r.confidence = {};\n",
                cypher_str(&source),
                cypher_str(&target),
                safe_rel(&relation),
                cypher_str(&confidence),
            ));
            count += 1;
        }
    }

    std::fs::write(out_path, script)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    #[test]
    fn writes_idempotent_cypher_script() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file, community) VALUES
              ('a', 'Alpha()', 'code', 'src/a.rs', 1),
              ('b', 'Beta()', 'code', 'src/b.rs', NULL);
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('a', 'b', 'calls', 'EXTRACTED', 'src/a.rs');
            ",
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("astria.cypher");
        let count = export_cypher(&db, &out).unwrap();
        assert_eq!(count, 3);

        let script = std::fs::read_to_string(&out).unwrap();
        assert!(script.contains("MERGE (n:Code {id: \"a\"})"));
        assert!(script.contains("n.community = 1"));
        assert!(script.contains("MERGE (a)-[r:CALLS]->(b) SET r.confidence = \"EXTRACTED\";"));
    }

    #[test]
    fn hostile_input_is_escaped() {
        let db = open_db_in_memory().unwrap();
        db.execute(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES (?1, ?2, 'code', 'f.rs')",
            rusqlite::params!["we\"ird\\id", "l\nabel"],
        )
        .unwrap();
        db.execute_batch(
            "INSERT INTO edges (source, target, relation, confidence, source_file)
             VALUES ('we\"ird\\id', 'we\"ird\\id', 'uses-stuff', 'INFERRED', 'f.rs');",
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("g.cypher");
        export_cypher(&db, &out).unwrap();
        let script = std::fs::read_to_string(&out).unwrap();
        // quotes and backslashes escaped, newline becomes \n literal
        assert!(script.contains("\"we\\\"ird\\\\id\""));
        assert!(script.contains("\"l\\nabel\""));
        // relation sanitized to an uppercased identifier
        assert!(script.contains("[r:USES_STUFF]->"));
    }

    #[test]
    fn safe_rel_normalizes() {
        assert_eq!(safe_rel("calls"), "CALLS");
        assert_eq!(safe_rel("uses-stuff"), "USES_STUFF");
        assert_eq!(safe_rel("??"), "RELATED_TO");
    }
}
