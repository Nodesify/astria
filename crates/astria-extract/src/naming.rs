// naming: stable identifier construction shared by every extractor.

use std::path::Path;

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

/// Return `parent_dir_stem/file_stem` for uniqueness.
pub(crate) fn file_stem(path: &Path) -> String {
    let file_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if parent.is_empty() {
        file_name.to_string()
    } else {
        format!("{}_{}", parent, file_name)
    }
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
}
