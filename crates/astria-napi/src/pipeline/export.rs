//! graph.json export: one consistent read snapshot of the whole graph.
use super::*;

pub fn export_json(db: &Connection, out_path: &Path) -> astria_core::Result<()> {
    // One read snapshot for every table: without a shared transaction a
    // concurrent publisher can land between the node and edge reads and the
    // exported graph would mix two builds.
    let snapshot = db.unchecked_transaction()?;
    export_json_snapshot(&snapshot, out_path)
}

fn export_json_snapshot(
    snapshot: &rusqlite::Transaction<'_>,
    out_path: &Path,
) -> astria_core::Result<()> {
    let db: &Connection = snapshot;
    let mut nodes = Vec::new();
    let mut stmt = db.prepare(
        "SELECT id, label, file_type, source_file, source_line, docstring, community, signature FROM nodes",
    )?;
    #[allow(clippy::type_complexity)]
    let node_rows: Vec<(
        String,
        String,
        String,
        String,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
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
                row.get(7)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (id, label, ft, sf, line, doc, comm, sig) in &node_rows {
        nodes.push(serde_json::json!({
            "id": id,
            "label": label,
            "file_type": ft,
            "source_file": sf,
            "source_line": line,
            "docstring": doc,
            "community": comm,
            "signature": sig,
        }));
    }

    let mut edges = Vec::new();
    let mut stmt = db.prepare(
        "SELECT source, target, relation, confidence, confidence_score, source_file, source_line, context FROM edges",
    )?;
    #[allow(clippy::type_complexity)]
    let edge_rows: Vec<(
        String,
        String,
        String,
        String,
        Option<f64>,
        String,
        Option<i64>,
        Option<String>,
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
                row.get(7)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // Evidence provenance rides with every edge: the line anchor and the
    // derived-pass ownership ('global'/'deep') are part of the exchange
    // format, matching what SQLite stores.
    for (src, tgt, rel, conf, score, sf, line, context) in &edge_rows {
        edges.push(serde_json::json!({
            "source": src,
            "target": tgt,
            "relation": rel,
            "confidence": conf,
            "confidence_score": score,
            "source_file": sf,
            "source_line": line,
            "context": context,
        }));
    }

    // Hyperedges (schema-compatible with the official astria consumer).
    let hyperedges: Vec<serde_json::Value> = match astria_build::hyperedges::load_all(db) {
        Ok(list) => list
            .iter()
            .map(|h| {
                serde_json::json!({
                    "id": h.id,
                    "label": h.label,
                    "nodes": h.nodes,
                    "relation": h.relation,
                    "confidence": h.confidence,
                    "confidence_score": h.score,
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    // Communities with their labels: thematic/hub fallback or LLM-named
    // (label_source='llm'), so agents reading graph.json get group intent,
    // not just group membership ids.
    let communities: Vec<serde_json::Value> = {
        let mut stmt = db.prepare(
            "SELECT id, label, summary, label_source, cohesion, size FROM communities ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(id, label, summary, source, cohesion, size)| {
                serde_json::json!({
                    "id": id,
                    "label": label,
                    "summary": summary,
                    "label_source": source,
                    "cohesion": cohesion,
                    "size": size,
                })
            })
            .collect()
    };

    // Publication metadata: the generation stamped for this build rides with
    // the export so consumers can detect mismatched artifact/database pairs.
    let meta: serde_json::Map<String, serde_json::Value> = {
        let mut stmt = db.prepare("SELECT key, value FROM _meta")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect()
    };

    let graph = serde_json::json!({
        "nodes": nodes,
        "edges": edges,
        "hyperedges": hyperedges,
        "communities": communities,
        "_meta": meta,
    });
    let json = serde_json::to_string_pretty(&graph)?;
    write_artifact_atomic(out_path, json.as_bytes())?;
    Ok(())
}
