// neo4j_push: live Bolt push of the graph into a Neo4j instance — the
// companion to the Cypher *file* export. Same data, same idempotent MERGE
// semantics, no cypher-shell required. Constraint creation is best-effort
// so older servers degrade to plain MERGEs instead of failing the push.

use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::packstream::Value;
use crate::BoltClient;
use astria_core::Result;

/// Rows per UNWIND statement — large enough to amortize round-trips,
/// small enough to stay well inside transaction memory limits.
const BATCH: usize = 500;

pub struct PushCounts {
    pub nodes: usize,
    pub edges: usize,
    pub communities: usize,
    pub statements: usize,
}

/// Uppercase relation names into valid Cypher relationship types — the
/// same shape the Cypher file export emits, so both routes build the same
/// graph.
pub fn safe_rel(relation: &str) -> String {
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
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out = format!("RELATED_{out}");
    }
    out
}

/// The `$rows` parameter map every UNWIND statement takes.
fn rows_param(rows: Vec<BTreeMap<String, Value>>) -> BTreeMap<String, Value> {
    let mut params = BTreeMap::new();
    params.insert(
        "rows".into(),
        Value::List(rows.into_iter().map(Value::Map).collect()),
    );
    params
}

/// Push the whole graph. Idempotent: MERGE on stable ids means re-running
/// updates properties in place instead of duplicating.
pub fn neo4j_push(db: &Connection, url: &str, user: &str, pass: &str) -> Result<PushCounts> {
    let mut client = BoltClient::connect(url, user, pass)?;

    // Uniqueness constraint makes the MERGEs safe under concurrent pushes.
    // Servers without the syntax keep working — the push proceeds either way.
    let mut statements = 0usize;
    if client
        .run(
            "CREATE CONSTRAINT astria_symbol_id IF NOT EXISTS FOR (s:Symbol) REQUIRE s.id IS UNIQUE",
            BTreeMap::new(),
        )
        .is_ok()
    {
        statements += 1;
    }

    // Nodes.
    let node_count: usize = {
        let mut stmt = db.prepare(
            "SELECT id, label, file_type, COALESCE(community, -1)
             FROM nodes ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        let mut batch: Vec<BTreeMap<String, Value>> = Vec::with_capacity(BATCH);
        for row in rows.filter_map(|r| r.ok()) {
            let (id, label, file_type, community) = row;
            let mut map = BTreeMap::new();
            map.insert("id".into(), Value::String(id));
            map.insert("label".into(), Value::String(label));
            map.insert("nodeType".into(), Value::String(file_type));
            map.insert("community".into(), Value::Integer(community));
            batch.push(map);
            if batch.len() == BATCH {
                client.run(UNWIND_NODE, rows_param(std::mem::take(&mut batch)))?;
                statements += 1;
            }
        }
        if !batch.is_empty() {
            client.run(UNWIND_NODE, rows_param(batch))?;
            statements += 1;
        }
        // Count separately from the cursor so the value survives the borrow.
        db.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    };

    // Edges, grouped by relation so each Cypher relationship type stays
    // honest (CALLS edges are CALLS in Neo4j, not a generic REL).
    let edge_count: usize = {
        let mut stmt =
            db.prepare("SELECT relation, source, target, confidence FROM edges ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let mut total = 0usize;
        let mut current_rel: Option<String> = None;
        let mut batch: Vec<BTreeMap<String, Value>> = Vec::with_capacity(BATCH);
        for row in rows.filter_map(|r| r.ok()) {
            let (relation, source, target, confidence) = row;
            let rel_type = safe_rel(&relation);
            if current_rel.as_deref() != Some(rel_type.as_str()) && !batch.is_empty() {
                // A non-empty batch implies a relation was already seen, so
                // the prior relation is always present here.
                let prior = current_rel
                    .as_deref()
                    .expect("non-empty batch implies a current relation");
                client.run(&unwind_edge(prior), rows_param(std::mem::take(&mut batch)))?;
                statements += 1;
            }
            current_rel = Some(rel_type);
            let mut map = BTreeMap::new();
            map.insert("source".into(), Value::String(source));
            map.insert("target".into(), Value::String(target));
            map.insert("confidence".into(), Value::String(confidence));
            batch.push(map);
            total += 1;
            if batch.len() == BATCH {
                // The row that pushed this batch set `current_rel` first.
                let prior = current_rel
                    .as_deref()
                    .expect("batched rows always set a current relation");
                client.run(&unwind_edge(prior), rows_param(std::mem::take(&mut batch)))?;
                statements += 1;
            }
        }
        if !batch.is_empty() {
            client.run(
                &unwind_edge(current_rel.as_deref().unwrap_or("RELATED_TO")),
                rows_param(batch),
            )?;
            statements += 1;
        }
        total
    };

    // Communities as first-class nodes (labels + summaries ride along).
    let community_count: usize = {
        let mut stmt = db.prepare(
            "SELECT id, label, COALESCE(summary, ''), label_source, size FROM communities ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        let mut count = 0usize;
        let mut batch: Vec<BTreeMap<String, Value>> = Vec::with_capacity(BATCH);
        for row in rows.filter_map(|r| r.ok()) {
            let (id, label, summary, label_source, size) = row;
            let mut map = BTreeMap::new();
            map.insert("id".into(), Value::Integer(id));
            map.insert("label".into(), Value::String(label));
            map.insert("summary".into(), Value::String(summary));
            map.insert("labelSource".into(), Value::String(label_source));
            map.insert("size".into(), Value::Integer(size));
            batch.push(map);
            count += 1;
        }
        if !batch.is_empty() {
            client.run(UNWIND_COMMUNITY, rows_param(batch))?;
            statements += 1;
        }
        count
    };

    Ok(PushCounts {
        nodes: node_count,
        edges: edge_count,
        communities: community_count,
        statements,
    })
}

const UNWIND_NODE: &str =
    "UNWIND $rows AS row MERGE (n:Symbol {id: row.id}) SET n.label = row.label, n.nodeType = row.nodeType, n.community = row.community";
const UNWIND_COMMUNITY: &str =
    "UNWIND $rows AS row MERGE (c:Community {id: row.id}) SET c.label = row.label, c.summary = row.summary, c.labelSource = row.labelSource, c.size = row.size";

fn unwind_edge(rel_type: &str) -> String {
    format!(
        "UNWIND $rows AS row MATCH (a:Symbol {{id: row.source}}), (b:Symbol {{id: row.target}}) MERGE (a)-[r:{rel_type} {{relation: row.relation}}]->(b) SET r.confidence = row.confidence"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_rel_matches_cypher_export_shape() {
        assert_eq!(safe_rel("calls"), "CALLS");
        assert_eq!(safe_rel("similar_to"), "SIMILAR_TO");
        assert_eq!(safe_rel("participate-in"), "PARTICIPATE_IN");
        assert_eq!(safe_rel(""), "RELATED_");
        assert_eq!(
            safe_rel("1hop"),
            "RELATED_1HOP",
            "cannot start with a digit"
        );
    }

    #[test]
    fn edge_statement_is_parameterized_and_typed() {
        let sql = unwind_edge("CALLS");
        assert!(sql.contains("MERGE (a)-[r:CALLS {relation: row.relation}]->(b)"));
        assert!(
            sql.contains("$rows"),
            "rows ride the parameter, never string-interpolated"
        );
    }
}
