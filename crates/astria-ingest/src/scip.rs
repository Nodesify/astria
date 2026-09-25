// scip: ingest a simplified SCIP-style JSON index (documents[] / symbols[]
// / relationships[] — the same shape upstream astria accepts, not the full
// protobuf) into an Extraction. Fully local, stdlib JSON only. Symbols from
// rust-analyzer/SCIP-producing toolchains join the graph with scip_impl /
// scip_typed / scip_def / scip_ref edges; unresolved targets become stubs.

use std::path::Path;

use sha2::Digest;

use astria_core::ids::normalize_id;
use astria_core::AstriaError;
use astria_core::Result;
use astria_extract::{ExtractedEdge, ExtractedNode, Extraction};

/// Deterministic node id for a (scip file, symbol) pair, mirroring upstream's
/// `scip_{suffix}_{hash[:12]}` shape (SHA-256 via the shared sha2 crate).
fn scip_node_id(doc_name: &str, symbol: &str) -> String {
    let doc_suffix = normalize_id(
        Path::new(doc_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("doc"),
    );
    let mut hasher = sha2::Sha256::new();
    hasher.update(doc_name.as_bytes());
    hasher.update([0u8]);
    hasher.update(symbol.as_bytes());
    let digest = hasher.finalize();
    let mut id = format!("scip_{doc_suffix}_");
    for b in &digest[..6] {
        id.push_str(&format!("{b:02x}"));
    }
    id
}

/// Parse a simplified SCIP JSON file into an Extraction keyed to a virtual
/// source file (the scip path itself), so rebuilds replace cleanly.
pub fn parse_scip(scip_path: &Path, text: &str) -> Result<Extraction> {
    let doc: serde_json::Value = serde_json::from_str(text).map_err(|e| AstriaError::Parse {
        file: scip_path.display().to_string(),
        message: format!("invalid SCIP JSON: {e}"),
    })?;

    let documents = doc
        .get("documents")
        .and_then(|d| d.as_array())
        .ok_or_else(|| AstriaError::Parse {
            file: scip_path.display().to_string(),
            message: "missing `documents` array".into(),
        })?;

    let mut nodes: Vec<ExtractedNode> = Vec::new();
    let mut edges: Vec<ExtractedEdge> = Vec::new();
    let virtual_file = scip_path.to_path_buf();

    let empty: Vec<serde_json::Value> = Vec::new();

    for document in documents {
        let doc_name = document
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or("unknown.scip")
            .to_string();
        let symbols = document
            .get("symbols")
            .and_then(|s| s.as_array())
            .unwrap_or(&empty);
        for symbol in symbols {
            let symbol_id = symbol
                .get("symbol")
                .and_then(|s| s.as_str())
                .unwrap_or_default()
                .to_string();
            if symbol_id.is_empty() {
                continue;
            }
            let display = symbol
                .get("display_name")
                .and_then(|s| s.as_str())
                .unwrap_or_else(|| symbol_id.rsplit('/').next().unwrap_or(&symbol_id))
                .to_string();
            let id = scip_node_id(&doc_name, &symbol_id);
            if !nodes.iter().any(|n| n.id == id) {
                nodes.push(ExtractedNode {
                    id: id.clone(),
                    label: display,
                    source_file: virtual_file.clone(),
                    source_line: Some(1),
                    docstring: None,
                    signature: None,
                    node_type: "code".to_string(),
                });
            }

            let rels = symbol
                .get("relationships")
                .and_then(|r| r.as_array())
                .unwrap_or(&empty);
            for rel in rels {
                let target_symbol = rel
                    .get("symbol")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default();
                if target_symbol.is_empty() || target_symbol == symbol_id {
                    continue;
                }
                let target_id = scip_node_id(&doc_name, target_symbol);
                let relation = if rel.get("is_implementation").and_then(|b| b.as_bool())
                    == Some(true)
                {
                    "scip_impl"
                } else if rel.get("is_type_definition").and_then(|b| b.as_bool()) == Some(true) {
                    "scip_typed"
                } else if rel.get("is_reference").and_then(|b| b.as_bool()) == Some(true) {
                    "scip_ref"
                } else {
                    "scip_def"
                };
                edges.push(ExtractedEdge {
                    source: id.clone(),
                    target: target_id,
                    relation: relation.to_string(),
                    confidence: "EXTRACTED".to_string(),
                    confidence_score: Some(1.0),
                    source_file: virtual_file.clone(),
                    source_line: Some(1),
                });
            }
        }
    }

    if nodes.is_empty() {
        return Err(AstriaError::Parse {
            file: scip_path.display().to_string(),
            message: "no symbols found in SCIP JSON".into(),
        });
    }

    Ok(Extraction {
        file_path: virtual_file,
        language: "SCIP".to_string(),
        nodes,
        edges,
    })
}

/// Parse a SCIP index file from disk.
pub fn parse_scip_file(scip_path: &Path) -> Result<Extraction> {
    let text = std::fs::read_to_string(scip_path).map_err(|e| AstriaError::Parse {
        file: scip_path.display().to_string(),
        message: format!("read failed: {e}"),
    })?;
    parse_scip(scip_path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scip_round_trips_symbols_and_relationships() {
        let json = r#"{
            "documents": [{
                "path": "src/lib.rs",
                "language": "rust",
                "symbols": [
                    {"symbol": "astria::Graph", "display_name": "Graph", "kind": "Class",
                     "relationships": [
                        {"symbol": "astria::GraphLike", "is_implementation": true},
                        {"symbol": "astria::Node", "is_type_definition": true}
                     ]},
                    {"symbol": "astria::GraphLike", "display_name": "GraphLike"}
                ]
            }]
        }"#;
        let ext = parse_scip(Path::new("index.scip.json"), json).unwrap();
        // Two symbol entries; astria::Node only appears as a relationship
        // target and materializes as a stub during build.
        assert_eq!(ext.nodes.len(), 2);
        assert!(ext.nodes.iter().any(|n| n.label == "Graph"));
        // Both relationship targets resolve to the SAME id for the same
        // symbol (deterministic hash), so impl + typed edges point at
        // astria::GraphLike's node.
        let impl_edge = ext
            .edges
            .iter()
            .find(|e| e.relation == "scip_impl")
            .unwrap();
        let typed_edge = ext
            .edges
            .iter()
            .find(|e| e.relation == "scip_typed")
            .unwrap();
        assert_ne!(impl_edge.target, impl_edge.source);
        // scip_typed points at astria::Node — a different symbol, so a
        // different id from the impl target.
        assert_ne!(typed_edge.target, impl_edge.target);
        assert_ne!(typed_edge.target, typed_edge.source);
        // Stable ids across parses.
        let again = parse_scip(Path::new("index.scip.json"), json).unwrap();
        for (a, b) in ext.nodes.iter().zip(again.nodes.iter()) {
            assert_eq!(a.id, b.id, "node ids diverged: {} vs {}", a.label, b.label);
            assert_eq!(a.label, b.label);
        }
        assert_eq!(ext.nodes.len(), again.nodes.len());
    }

    #[test]
    fn scip_rejects_garbage() {
        assert!(parse_scip(Path::new("x.json"), "not json").is_err());
        assert!(parse_scip(Path::new("x.json"), r#"{"foo": 1}"#).is_err());
    }
}
