// VB.NET: regex-fallback extraction.
//
// No tree-sitter grammar crate exists for VB.NET, so (like graphify's own
// Pascal fallback) we scan declaration lines directly. We extract
// Class/Module/Interface/Structure containers and Sub/Function members with
// `contains` edges; imports (Imports X) are captured as import nodes.

use std::fs;
use std::path::Path;

use astria_core::{AstriaError, Result};
use regex::Regex;

use crate::naming::{file_stem, make_node_id};
use crate::schema::{ExtractedEdge, ExtractedNode, Extraction};

/// Config placeholder so the registry/docs generator can see the language.
/// Extraction never goes through the tree-sitter walker (no grammar exists);
/// the engine routes `.vb` to [`extract_regex`] directly.
pub fn config() -> &'static crate::langs::config::LanguageConfig {
    static CONFIG: crate::langs::config::LanguageConfig = crate::langs::config::LanguageConfig {
        name: astria_core::languages::LanguageId::VbNet
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::VbNet
            .registration()
            .extensions,
        language_fn: || crate::langs::config::missing_language("vb.net"),
        compiled_in: false,
        class_types: &[],
        function_types: &[],
        import_types: &[],
        call_type: "",
        name_child: None,
        name_field: "name",
        body_field: None,
        body_fallback_types: &[],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}

static VB_CONTAINER_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:Public|Private|Protected|Friend|Partial|MustInherit|NotInheritable|Shadows|\s)*\b(Class|Module|Interface|Structure)\s+([A-Za-z_]\w*)",
    )
    .expect("static regex")
});
static VB_MEMBER_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:Public|Private|Protected|Friend|Shared|Overrides|Overloads|Overridable|MustOverride|NotOverridable|Partial|Default|ReadOnly|WriteOnly|WithEvents|Dim|Const|\s)*\b(Sub|Function)\s+([A-Za-z_]\w*)",
    )
    .expect("static regex")
});
static VB_IMPORT_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(?i)^\s*Imports\s+([A-Za-z_][\w.]*)").expect("static regex")
});
static VB_END_BLOCK_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(?i)^\s*End\s+(Class|Module|Interface|Structure)\b").expect("static regex")
});

/// Regex-based VB.NET extraction: containers, members, imports.
pub fn extract_regex(path: &Path, naming: &Path) -> Result<Extraction> {
    let source = fs::read_to_string(path).map_err(|e| {
        AstriaError::Io(std::io::Error::new(
            e.kind(),
            format!("Cannot read VB file {}: {e}", path.display()),
        ))
    })?;

    let fid = file_stem(naming);
    let file_id = make_node_id(&[&fid]);

    let mut nodes = vec![ExtractedNode {
        id: file_id.clone(),
        label: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string(),
        source_file: path.to_path_buf(),
        source_line: None,
        docstring: None,
        signature: None,
        node_type: "file".to_string(),
    }];
    let mut edges = Vec::new();

    // Stack of open container ids; `End Class` & friends pop one level.
    let mut stack: Vec<String> = Vec::new();

    for (index, line) in source.lines().enumerate() {
        let lineno = index as u32 + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if VB_END_BLOCK_RE.is_match(trimmed) {
            stack.pop();
            continue;
        }

        if let Some(caps) = VB_CONTAINER_RE.captures(trimmed) {
            let name = caps[2].to_string();
            let parent = stack.last().cloned().unwrap_or_else(|| file_id.clone());
            let id = make_node_id(&[&parent, &name]);
            nodes.push(ExtractedNode {
                id: id.clone(),
                label: name.clone(),
                source_file: path.to_path_buf(),
                source_line: Some(lineno),
                docstring: None,
                signature: Some(collapse(trimmed)),
                node_type: "class".to_string(),
            });
            edges.push(ExtractedEdge {
                source: parent,
                target: id.clone(),
                relation: "contains".to_string(),
                confidence: "EXTRACTED".to_string(),
                confidence_score: None,
                source_file: path.to_path_buf(),
                source_line: Some(lineno),
            });
            stack.push(id);
            continue;
        }

        if let Some(caps) = VB_MEMBER_RE.captures(trimmed) {
            let name = caps[2].to_string();
            let parent = stack.last().cloned().unwrap_or_else(|| file_id.clone());
            let id = make_node_id(&[&parent, &name]);
            nodes.push(ExtractedNode {
                id: id.clone(),
                label: format!("{name}()"),
                source_file: path.to_path_buf(),
                source_line: Some(lineno),
                docstring: None,
                signature: Some(collapse(trimmed)),
                node_type: "function".to_string(),
            });
            edges.push(ExtractedEdge {
                source: parent,
                target: id,
                relation: "contains".to_string(),
                confidence: "EXTRACTED".to_string(),
                confidence_score: None,
                source_file: path.to_path_buf(),
                source_line: Some(lineno),
            });
            continue;
        }

        if let Some(caps) = VB_IMPORT_RE.captures(trimmed) {
            let target = caps[1].to_string();
            edges.push(ExtractedEdge {
                source: file_id.clone(),
                target,
                relation: "imports".to_string(),
                confidence: "EXTRACTED".to_string(),
                confidence_score: None,
                source_file: path.to_path_buf(),
                source_line: Some(lineno),
            });
        }
    }

    Ok(Extraction {
        file_path: path.to_path_buf(),
        language: "VB.NET".to_string(),
        nodes,
        edges,
    })
}

fn collapse(line: &str) -> String {
    let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(160).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    use astria_core::db::open_db_in_memory;

    #[test]
    fn vb_containers_members_and_imports() {
        let source = "Imports System.Collections.Generic\r\n\r\nPublic Class Greeter\r\n    Private ReadOnly prefix As String\r\n\r\n    Public Sub New(prefix As String)\r\n        Me.prefix = prefix\r\n    End Sub\r\n\r\n    Public Function Greet(name As String) As String\r\n        Return prefix & name\r\n    End Function\r\nEnd Class\r\n\r\nFriend Module Program\r\n    Sub Main()\r\n        Dim g As New Greeter(\"hi \")\r\n    End Sub\r\nEnd Module\r\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Greeter.vb");
        fs::write(&file, source).unwrap();
        let db = open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        let ext = results.remove(0);

        assert_eq!(ext.language, "VB.NET");
        let labels: Vec<&str> = ext.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(labels.contains(&"Greeter"), "class: {labels:?}");
        assert!(labels.contains(&"Program"), "module: {labels:?}");
        assert!(labels.contains(&"Greet()"), "function: {labels:?}");
        assert!(labels.contains(&"New()"), "ctor sub: {labels:?}");
        assert!(labels.contains(&"Main()"), "sub: {labels:?}");
        assert!(ext
            .edges
            .iter()
            .any(|e| e.relation == "imports" && e.target == "System.Collections.Generic"));
        assert!(ext.edges.iter().any(|e| e.relation == "contains"
            && e.source.to_lowercase().ends_with("greeter")
            && e.target.to_lowercase().ends_with("greet")));
    }

    #[test]
    fn vb_nested_container_nests_members() {
        let source = "Namespace Outer\r\n    Class Inner\r\n        Sub Run()\r\n        End Sub\r\n    End Class\r\nEnd Namespace\r\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Inner.vb");
        fs::write(&file, source).unwrap();
        let db = open_db_in_memory().unwrap();
        let results = engine::extract(&[file], dir.path(), &db).unwrap();
        let ext = &results[0];
        // `Namespace` is not one of our containers; Inner still lands under
        // the file, Run under Inner.
        assert!(ext
            .edges
            .iter()
            .any(|e| e.relation == "contains" && e.target.to_lowercase().ends_with("run")));
    }
}
