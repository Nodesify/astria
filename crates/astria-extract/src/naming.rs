// naming: stable identifier construction shared by every extractor.

use std::collections::HashMap;
use std::path::{Component, Path};

use astria_core::ids::{normalize_id, normalize_id_case_preserved};
use sha2::{Digest, Sha256};

/// Join parts with `::` for hierarchical node IDs (e.g. "src_lib::greeter::greet").
/// Each part goes through case-preserving identity normalization: word
/// characters are kept with their original case (so case-distinct
/// declarations `Foo`/`foo` are different symbols), while punctuation and
/// Unicode compatibility forms still normalize. Search text and reference
/// matching use the case-folded [`make_target_id`] — structural identity and
/// normalized search text stay separate concerns.
pub(crate) fn make_node_id(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| normalize_id_case_preserved(p))
        .collect::<Vec<_>>()
        .join("::")
}

/// Create a target ID for cross-file references (imports, calls).
/// Qualified names (`pipeline::load_graph_db`, `PathBuf::from`) keep their
/// `::` segment structure so they can match hierarchical definition ids;
/// each segment is normalized for fuzzy matching.
pub(crate) fn make_target_id(name: &str) -> String {
    name.split("::")
        .map(normalize_id)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("::")
}

/// Short digest (12 hex chars) of the forward-slash relative path — the
/// collision-resistant part of file ids. Flattening components with `_`
/// alone cannot distinguish `a/b_c.rs` from `a_b/c.rs`, and lowercasing
/// cannot distinguish case-distinct paths (`Foo.ts`/`foo.ts` on
/// case-insensitive filesystems are different files); the digest of the
/// complete relative path can. 48 bits keeps collision odds negligible
/// (~10⁻⁷ at 10k files).
fn path_digest(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let hash = Sha256::digest(normalized.as_bytes());
    hash.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Collision-free id stem for a file: the complete relative path — every
/// component, extension included — flattened with `_`, plus a digest of the
/// full path. `src/lib.rs` → `src_lib_rs_<digest>`, so `src/foo.ts` and
/// `src/foo.js`, `a/b_c.rs` and `a_b/c.rs`, and case-distinct paths all get
/// distinct stems. The id prefix comes from the path relative to the
/// scanned root, so identical trees produce identical ids anywhere.
pub(crate) fn file_stem(path: &Path) -> String {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let dirs: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(dir) => dir.to_str(),
            _ => None,
        })
        .collect();
    let mut parts: Vec<&str> = dirs[..dirs.len().saturating_sub(1)].to_vec();
    parts.push(file_name);
    let flattened = parts.join("_");
    format!("{flattened}_{}", path_digest(path))
}

/// Same-file nodes whose ids still collided after case-preserving
/// normalization (identical spellings in one file, NFKC edge cases) get
/// deterministic disambiguators: the first keeps the id, later ones become
/// `<id>__2`, `<id>__3`… in declaration order. Edges that referenced the
/// colliding id are rewired to the definition they belong to: the duplicate
/// whose declaration line is the nearest one at or above the edge's own
/// source line (fallback: the first definition when the edge carries no
/// line). Idempotent: re-running on already-unique ids changes nothing.
pub(crate) fn disambiguate_duplicate_ids(extraction: &mut crate::schema::Extraction) {
    /// One definition sharing a base id: its rename (None = keeps the bare
    /// id, i.e. the first definition) and its declaration line.
    type Def = (Option<String>, Option<u32>);
    // base id -> definitions under it, in declaration order. The first
    // entry stays `None` (keeps the bare id).
    let mut renames: HashMap<String, Vec<Def>> = HashMap::new();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    for node in &mut extraction.nodes {
        if used.insert(node.id.clone()) {
            renames.insert(node.id.clone(), vec![(None, node.source_line)]);
            continue;
        }
        let base = node.id.clone();
        let mut k = 2;
        let mut candidate = format!("{base}__{k}");
        while !used.insert(candidate.clone()) {
            k += 1;
            candidate = format!("{base}__{k}");
        }
        let decl_line = node.source_line;
        node.id = candidate.clone();
        renames
            .entry(base)
            .or_default()
            .push((Some(candidate), decl_line));
    }

    // Rewire edge endpoints that still name a colliding base id onto the
    // definition the edge actually belongs to. Without this every edge of
    // the later definitions silently pointed at the first one.
    let resolve = |id: &str, edge_line: Option<u32>| -> String {
        let Some(defs) = renames.get(id) else {
            return id.to_string();
        };
        let pick = match edge_line {
            Some(line) => defs
                .iter()
                .rfind(|(_, decl)| decl.is_some_and(|d| d <= line))
                .or_else(|| defs.first()),
            None => defs.first(),
        };
        match pick {
            Some((Some(renamed), _)) => renamed.clone(),
            _ => id.to_string(),
        }
    };
    for edge in &mut extraction.edges {
        edge.source = resolve(&edge.source, edge.source_line);
        edge.target = resolve(&edge.target, edge.source_line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn node_ids_are_joined_and_case_preserving() {
        // Word characters keep their case (structural identity), punctuation
        // still normalizes; empty/blank parts drop out.
        assert_eq!(
            make_node_id(&["Src Lib", "Greeter", "greet()"]),
            "Src_Lib::Greeter::greet"
        );
        assert_eq!(make_node_id(&["", "a", "  "]), "a");
    }

    #[test]
    fn case_distinct_declarations_get_distinct_ids() {
        // `Foo` and `foo` in one file are different symbols now — no
        // collision, no disambiguator, no folded identity.
        assert_ne!(
            make_node_id(&["a.py", "Foo"]),
            make_node_id(&["a.py", "foo"])
        );
    }

    #[test]
    fn target_ids_keep_segment_structure() {
        assert_eq!(
            make_target_id("pipeline::load_graph_db"),
            "pipeline::load_graph_db"
        );
        assert_eq!(make_target_id("PathBuf::from"), "pathbuf::from");
    }

    #[test]
    fn file_stem_includes_parent_dir_and_extension() {
        // `src/lib.rs` → id prefix `src_lib_rs_<digest>`; `main.py` →
        // `main_py_<digest>` (the dot normalizes away in make_node_id).
        assert!(
            make_node_id(&[&file_stem(&PathBuf::from("src/lib.rs"))]).starts_with("src_lib_rs_")
        );
        assert!(make_node_id(&[&file_stem(&PathBuf::from("main.py"))]).starts_with("main_py_"));
    }

    #[test]
    fn workspace_file_ids_do_not_collide() {
        let analyze = PathBuf::from("crates/astria-analyze/src/lib.rs");
        let cluster = PathBuf::from("crates/astria-cluster/src/lib.rs");
        assert_ne!(file_stem(&analyze), file_stem(&cluster));
    }

    #[test]
    fn extension_and_flattening_collisions_are_distinct() {
        // Same stem, different extensions: two different files.
        assert_ne!(
            file_stem(&PathBuf::from("src/foo.ts")),
            file_stem(&PathBuf::from("src/foo.js"))
        );
        // `_`-flattening collision: same components rearranged.
        assert_ne!(
            file_stem(&PathBuf::from("a/b_c.rs")),
            file_stem(&PathBuf::from("a_b/c.rs"))
        );
        // Case-distinct paths (case-insensitive filesystems) are different
        // files even though normalize_id lowercases.
        assert_ne!(
            file_stem(&PathBuf::from("src/Foo.ts")),
            file_stem(&PathBuf::from("src/foo.ts"))
        );
    }

    #[test]
    fn file_stems_are_stable_for_identical_relative_paths() {
        assert_eq!(
            file_stem(&PathBuf::from("deep/nest/thing.rs")),
            file_stem(&PathBuf::from("deep/nest/thing.rs"))
        );
    }

    #[test]
    fn case_distinct_declarations_get_disambiguated() {
        use crate::schema::{ExtractedEdge, ExtractedNode, Extraction};
        let mut ext = Extraction {
            file_path: PathBuf::from("a.py"),
            language: "Python".into(),
            nodes: vec![
                ExtractedNode {
                    id: "a_py::foo".into(),
                    label: "foo".into(),
                    source_file: PathBuf::from("a.py"),
                    source_line: Some(1),
                    docstring: None,
                    signature: None,
                    node_type: "class".into(),
                },
                ExtractedNode {
                    id: "a_py::foo".into(),
                    label: "foo".into(),
                    source_file: PathBuf::from("a.py"),
                    source_line: Some(9),
                    docstring: None,
                    signature: None,
                    node_type: "function".into(),
                },
                ExtractedNode {
                    id: "a_py::bar".into(),
                    label: "bar".into(),
                    source_file: PathBuf::from("a.py"),
                    source_line: Some(11),
                    docstring: None,
                    signature: None,
                    node_type: "function".into(),
                },
            ],
            edges: vec![
                // A call at line 10 sits inside the SECOND definition.
                ExtractedEdge {
                    source: "a_py::foo".into(),
                    target: "a_py::bar".into(),
                    relation: "calls".into(),
                    confidence: "INFERRED".into(),
                    confidence_score: None,
                    source_file: PathBuf::from("a.py"),
                    source_line: Some(10),
                },
                // An edge above every declaration, or without a line, stays
                // on the first definition.
                ExtractedEdge {
                    source: "a_py::foo".into(),
                    target: "a_py::bar".into(),
                    relation: "calls".into(),
                    confidence: "INFERRED".into(),
                    confidence_score: None,
                    source_file: PathBuf::from("a.py"),
                    source_line: Some(5),
                },
                ExtractedEdge {
                    source: "a_py::foo".into(),
                    target: "a_py::bar".into(),
                    relation: "calls".into(),
                    confidence: "INFERRED".into(),
                    confidence_score: None,
                    source_file: PathBuf::from("a.py"),
                    source_line: None,
                },
            ],
        };
        disambiguate_duplicate_ids(&mut ext);
        let ids: Vec<&str> = ext.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["a_py::foo", "a_py::foo__2", "a_py::bar"]);
        // Edge at line 10 follows the definition declared at line 9; the
        // line-5 and line-less edges keep the first definition.
        assert_eq!(ext.edges[0].source, "a_py::foo__2");
        assert_eq!(ext.edges[1].source, "a_py::foo");
        assert_eq!(ext.edges[2].source, "a_py::foo");
        // Rewiring is idempotent: the second run sees unique ids.
        let before: Vec<String> = ext.edges.iter().map(|e| e.source.clone()).collect();
        disambiguate_duplicate_ids(&mut ext);
        let after: Vec<String> = ext.edges.iter().map(|e| e.source.clone()).collect();
        assert_eq!(before, after);
    }
}
