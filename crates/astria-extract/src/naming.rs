// naming: stable identifier construction shared by every extractor.

use std::path::{Component, Path};

use astria_core::ids::normalize_id;

/// Join parts with `::` for hierarchical node IDs (e.g. "src_lib::greeter::greet").
/// Each part goes through `normalize_id` (casefold+NFKC stable), so identical
/// entities always produce identical ids regardless of source casing or
/// Unicode compatibility forms.
pub(crate) fn make_node_id(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| normalize_id(p))
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

/// Collision-free id stem for a file: every path component (extension
/// dropped) joined with `_`, so ids stay unique across a workspace.
/// `src/lib.rs` → `src_lib` (flat projects keep their old ids), while
/// `crates/a/src/lib.rs` → `crates_a_src_lib` instead of colliding with
/// every other crate's `src/lib.rs` — the collision that used to funnel
/// every crate's `contains` edges into one shared `src_lib` hub node.
pub(crate) fn file_stem(path: &Path) -> String {
    let stem = path
        .file_stem()
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
    parts.push(stem);
    parts.join("_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn node_ids_are_joined_and_normalized() {
        assert_eq!(
            make_node_id(&["Src Lib", "Greeter", "greet()"]),
            "src_lib::greeter::greet"
        );
        assert_eq!(make_node_id(&["", "a", "  "]), "a");
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
    fn file_stem_includes_parent_dir() {
        assert_eq!(file_stem(&PathBuf::from("src/lib.rs")), "src_lib");
        assert_eq!(file_stem(&PathBuf::from("main.py")), "main");
    }

    #[test]
    fn workspace_file_ids_do_not_collide() {
        // The stem used to keep only the last directory, so every crate's
        // src/lib.rs produced the same `src_lib` id and build() treated the
        // later crates' lib.rs as cross-file merges of the first — one hub
        // node absorbed the whole workspace's `contains` edges.
        let analyze = PathBuf::from("crates/astria-analyze/src/lib.rs");
        let cluster = PathBuf::from("crates/astria-cluster/src/lib.rs");
        assert_eq!(
            make_node_id(&[&file_stem(&analyze)]),
            "crates_astria_analyze_src_lib"
        );
        assert_ne!(file_stem(&analyze), file_stem(&cluster));
    }
}
