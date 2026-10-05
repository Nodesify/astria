// refs: cross-file reference resolution — matches call/import targets
// against all known node ids once every file has been extracted.

use crate::naming::make_target_id;
use crate::schema::Extraction;
use std::collections::{HashMap, HashSet};

/// Resolve names using complete scope suffixes before considering bare names.
/// Every candidate set stays ambiguous unless file metadata selects one definition.
/// These are name-derived bindings, never compiler-proven EXTRACTED calls.
pub(crate) fn resolve_cross_file_references(results: &mut [Extraction]) {
    let mut names: HashMap<String, Vec<&crate::schema::ExtractedNode>> = HashMap::new();
    let known_ids: HashSet<_> = results
        .iter()
        .flat_map(|ext| ext.nodes.iter().map(|node| node.id.as_str()))
        .collect();
    let mut modules: HashMap<String, Vec<&Extraction>> = HashMap::new();
    for ext in results.iter() {
        let mut keys = vec![
            module_path(&ext.file_path),
            module_path(&ext.file_path.with_extension("")),
        ];
        if let Some(stem) = ext.file_path.file_stem().and_then(|s| s.to_str()) {
            keys.push(stem.to_string());
        }
        keys.sort();
        keys.dedup();
        for key in keys {
            modules.entry(key).or_default().push(ext);
        }
        for node in &ext.nodes {
            // Only code definitions can be callees; prose and identifier-shaped
            // string references are not declarations.
            if !matches!(node.node_type.as_str(), "function" | "class" | "test") {
                continue;
            }
            let mut keys = vec![make_target_id(node.label.trim_end_matches("()"))];
            // The first id segment is a path digest, not a lexical scope.
            let segments: Vec<&str> = node.id.split("::").collect();
            // Assigned dotted declarations carry their semantic scope in the
            // label; their flattened structural-id tail is not a bare alias.
            if !node.label.contains('.') {
                for start in 1..segments.len() {
                    keys.push(make_target_id(&segments[start..].join("::")));
                }
            }
            keys.sort();
            keys.dedup();
            for key in keys {
                names.entry(key).or_default().push(node);
            }
        }
    }
    // Build all rewrites before mutating results so lookup references stay valid.
    let mut rewrites = Vec::new();
    for (file_index, ext) in results.iter().enumerate() {
        let imported_files: Vec<_> = ext
            .edges
            .iter()
            .filter(|edge| edge.relation == "imports")
            .filter_map(|edge| {
                let matches = module_candidates(&edge.target, ext, &modules);
                (matches.len() == 1).then(|| &matches[0].file_path)
            })
            .collect();
        for (edge_index, edge) in ext.edges.iter().enumerate() {
            if edge.relation != "calls" && edge.relation != "imports" {
                continue;
            }
            // Preserve bindings already pointing to a complete structural id.
            if known_ids.contains(edge.target.as_str()) {
                continue;
            }
            let target = make_target_id(edge.target.trim_end_matches("()"));
            let mut candidates = names.get(&target).cloned().unwrap_or_default();
            if let Some((module, tail)) = target.split_once("::") {
                // Scope and module interpretations are equally possible. Union
                // both sets before applying local/import evidence; never let
                // a lexical scope hide a competing file-module definition.
                let files = module_candidates(module, ext, &modules);
                candidates.extend(
                    names
                        .get(tail)
                        .into_iter()
                        .flatten()
                        .filter(|node| files.iter().any(|file| node.source_file == file.file_path))
                        .copied(),
                );
            }
            candidates.sort_by(|a, b| a.id.cmp(&b.id));
            candidates.dedup_by(|a, b| a.id == b.id);
            if edge.relation == "imports" {
                // Module imports name files, not arbitrary same-name functions.
                candidates.clear();
                let files = module_candidates(&edge.target, ext, &modules);
                if files.len() == 1 {
                    candidates = files[0]
                        .nodes
                        .iter()
                        .filter(|node| node.node_type == "file")
                        .collect();
                }
            }
            let local: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|node| node.source_file == ext.file_path)
                .collect();
            let selected = if !local.is_empty() {
                unique_candidate(&local)
            } else if candidates.len() > 1 && target.contains("::") {
                // Module-only import metadata cannot safely bind bare aliases.
                let imported: Vec<_> = candidates
                    .iter()
                    .copied()
                    .filter(|node| imported_files.contains(&&node.source_file))
                    .collect();
                unique_candidate(&imported)
            } else {
                unique_candidate(&candidates)
            };
            if let Some(node) = selected {
                rewrites.push((file_index, edge_index, node.id.clone()));
            }
        }
    }
    for (file, edge_index, target) in rewrites {
        let edge = &mut results[file].edges[edge_index];
        edge.target = target;
        // The expression is extracted, but its rewritten endpoint is a name
        // binding. Direct structural-id edges were preserved by the guard above.
        edge.confidence = "RESOLVED".to_string();
        edge.confidence_score = Some(0.85);
    }
}

/// Lexical path normalization only; no filesystem lookup or alias guessing.
fn module_path(path: &std::path::Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for part in raw.split('/') {
        match part {
            "." => {}
            ".." if parts
                .last()
                .is_some_and(|last| *last != ".." && !last.is_empty()) =>
            {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

fn module_candidates<'a>(
    module: &str,
    caller: &Extraction,
    modules: &HashMap<String, Vec<&'a Extraction>>,
) -> Vec<&'a Extraction> {
    let key = if module.contains(['/', '\\']) {
        let path = std::path::Path::new(module);
        if path.is_absolute() {
            module_path(path)
        } else {
            module_path(
                &caller
                    .file_path
                    .parent()
                    .unwrap_or(std::path::Path::new(""))
                    .join(path),
            )
        }
    } else {
        module.to_string()
    };
    modules.get(&key).cloned().unwrap_or_default()
}

fn unique_candidate<'a>(
    nodes: &[&'a crate::schema::ExtractedNode],
) -> Option<&'a crate::schema::ExtractedNode> {
    (nodes.len() == 1).then(|| nodes[0])
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
        // A source-extracted call bound to exactly one definition is its
        // own tier: stronger than co-occurrence, below compiler-proven.
        assert_eq!(results[1].edges[0].confidence, "RESOLVED");
        assert_eq!(results[1].edges[0].confidence_score, Some(0.85));
    }

    #[test]
    fn resolved_binding_survives_re_resolution() {
        // Idempotence: an already-RESOLVED edge must not be re-labeled when
        // resolution runs again (update flows re-resolve cached edges).
        let mut results = vec![
            ext(vec![node("src_x::run", "run()")], vec![]),
            ext(vec![], vec![edge("caller", "run", "calls", "RESOLVED")]),
        ];
        resolve_cross_file_references(&mut results);
        assert_eq!(results[1].edges[0].target, "src_x::run");
        assert_eq!(results[1].edges[0].confidence, "RESOLVED");
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
