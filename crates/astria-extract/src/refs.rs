// refs: cross-file reference resolution — matches call/import targets
// against all known node ids once every file has been extracted.

use crate::schema::Extraction;
use std::collections::HashMap;

/// Build a lookup of all known node IDs (lowercased for matching) and try to
/// resolve INFERRED call edges to real node IDs. This turns stub references
/// into proper cross-file edges when a match is found.
///
/// Bare-name matches (call target "get" → definition "src_x::get") resolve
/// ONLY when exactly one node shares that bare name. When many definitions
/// share a name — `get`, `new`, `to_string` — picking one arbitrarily would
/// concentrate every call edge in the corpus onto a single node and mint a
/// fake god node.
pub(crate) fn resolve_cross_file_references(results: &mut [Extraction]) {
    // Collect all known node IDs and their labels
    let mut known_ids: HashMap<String, String> = HashMap::new();
    let mut bare_counts: HashMap<String, usize> = HashMap::new();
    let mut bare_ids: HashMap<String, String> = HashMap::new();
    for ext in results.iter() {
        for node in &ext.nodes {
            // Map: lowercase label -> actual node ID
            known_ids.insert(node.label.to_lowercase(), node.id.clone());
            // Also map by the last segment of the ID (e.g. "greet" from "main::Greeter::greet")
            let parts: Vec<&str> = node.id.split("::").collect();
            if let Some(last) = parts.last() {
                let lower = last.to_lowercase().trim_end_matches("()").to_string();
                *bare_counts.entry(lower.clone()).or_insert(0) += 1;
                bare_ids.entry(lower).or_insert_with(|| node.id.clone());
            }
        }
    }

    // Resolve edges
    for ext in results.iter_mut() {
        for edge in ext.edges.iter_mut() {
            if edge.relation == "calls" || edge.relation == "imports" {
                // Try the full target, then just its last segment — a call
                // `pipeline::load_graph_db()` targets "pipeline::load_graph_db"
                // but the definition is "src_pipeline::load_graph_db".
                let target_lower = edge.target.to_lowercase();
                let last_segment = edge
                    .target
                    .rsplit("::")
                    .next()
                    .unwrap_or(&edge.target)
                    .to_lowercase();
                let real_id = known_ids.get(&target_lower).or_else(|| {
                    // Bare-name resolution only when unambiguous.
                    if bare_counts.get(&last_segment) == Some(&1) {
                        bare_ids.get(&last_segment)
                    } else {
                        None
                    }
                });
                if let Some(real_id) = real_id {
                    if real_id != &edge.target {
                        edge.target = real_id.clone();
                        if edge.confidence == "INFERRED" {
                            edge.confidence = "EXTRACTED".to_string();
                            edge.confidence_score = Some(0.9);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ExtractedEdge, ExtractedNode};
    use std::path::PathBuf;

    fn ext(nodes: Vec<ExtractedNode>, edges: Vec<ExtractedEdge>) -> Extraction {
        Extraction {
            file_path: PathBuf::from("f.py"),
            language: "Python".into(),
            nodes,
            edges,
        }
    }

    fn node(id: &str, label: &str) -> ExtractedNode {
        ExtractedNode {
            id: id.into(),
            label: label.into(),
            source_file: PathBuf::from("f.py"),
            source_line: None,
            docstring: None,
            signature: None,
            node_type: "function".into(),
        }
    }

    fn edge(source: &str, target: &str, relation: &str, confidence: &str) -> ExtractedEdge {
        ExtractedEdge {
            source: source.into(),
            target: target.into(),
            relation: relation.into(),
            confidence: confidence.into(),
            confidence_score: Some(0.7),
            source_file: PathBuf::from("f.py"),
            source_line: None,
        }
    }

    #[test]
    fn bare_call_resolves_to_definition() {
        let mut results = vec![
            ext(vec![node("src_x::run", "run()")], vec![]),
            ext(vec![], vec![edge("caller", "run", "calls", "INFERRED")]),
        ];
        resolve_cross_file_references(&mut results);
        assert_eq!(results[1].edges[0].target, "src_x::run");
        assert_eq!(results[1].edges[0].confidence, "EXTRACTED");
        assert_eq!(results[1].edges[0].confidence_score, Some(0.9));
    }

    #[test]
    fn unresolved_targets_stay_inferred() {
        let mut results = vec![ext(
            vec![node("src_x::run", "run()")],
            vec![edge("caller", "nonexistent", "calls", "INFERRED")],
        )];
        resolve_cross_file_references(&mut results);
        assert_eq!(results[0].edges[0].target, "nonexistent");
        assert_eq!(results[0].edges[0].confidence, "INFERRED");
    }

    #[test]
    fn ambiguous_bare_names_stay_unresolved() {
        // Two definitions share the bare name "get" — a bare "get" call
        // must NOT collapse onto one of them (that mints a fake god node).
        let mut results = vec![
            ext(
                vec![node("src_a::get", "get()"), node("src_b::get", "get()")],
                vec![],
            ),
            ext(vec![], vec![edge("caller", "get", "calls", "INFERRED")]),
        ];
        resolve_cross_file_references(&mut results);
        assert_eq!(results[1].edges[0].target, "get");
        assert_eq!(results[1].edges[0].confidence, "INFERRED");
    }

    #[test]
    fn unique_bare_name_resolves_despite_other_labels() {
        let mut results = vec![
            ext(vec![node("src_a::load", "load()")], vec![]),
            ext(vec![], vec![edge("caller", "load", "calls", "INFERRED")]),
        ];
        resolve_cross_file_references(&mut results);
        assert_eq!(results[1].edges[0].target, "src_a::load");
    }
}
