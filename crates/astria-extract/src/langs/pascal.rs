// Pascal/Delphi: regex-fallback extraction.
//
// No tree-sitter grammar crate exists for Pascal; like graphify (which also
// ships Pascal as a regex extra) we scan declaration lines. Units, programs,
// classes (TP/Delphi `type TFoo = class`) and procedures/functions become
// nodes with `contains` edges.

use std::fs;
use std::path::Path;

use astria_core::{AstriaError, Result};
use regex::Regex;

use crate::naming::{file_stem, make_node_id};
use crate::schema::{ExtractedEdge, ExtractedNode, Extraction};

/// Config placeholder so the registry/docs generator can see the language.
/// Extraction never goes through the tree-sitter walker (no grammar exists);
/// the engine routes `.pas/.dpr/.dpk/.inc` to [`extract_regex`] directly.
pub fn config() -> &'static crate::langs::config::LanguageConfig {
    static CONFIG: crate::langs::config::LanguageConfig = crate::langs::config::LanguageConfig {
        name: astria_core::languages::LanguageId::Pascal
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Pascal
            .registration()
            .extensions,
        language_fn: || crate::langs::config::missing_language("pascal"),
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

static PASCAL_ROUTINE_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(procedure|function)\s+([A-Za-z_][\w.]*)").expect("static regex")
});
static PASCAL_UNIT_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(?i)^\s*unit\s+([A-Za-z_]\w*)").expect("static regex")
});
static PASCAL_PROGRAM_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(?i)^\s*program\s+([A-Za-z_]\w*)").expect("static regex")
});
static PASCAL_CLASS_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(?i)^\s*([A-Za-z_]\w*)\s*=\s*(?:packed\s+)?class\b").expect("static regex")
});
static PASCAL_USES_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(?i)^\s*uses\s+([A-Za-z_][\w.,\s]*)").expect("static regex")
});

/// Regex-based Pascal/Delphi extraction.
pub fn extract_regex(path: &Path, naming: &Path) -> Result<Extraction> {
    let source = fs::read_to_string(path).map_err(|e| {
        AstriaError::Io(std::io::Error::new(
            e.kind(),
            format!("Cannot read Pascal file {}: {e}", path.display()),
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

    for (index, line) in source.lines().enumerate() {
        let lineno = index as u32 + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('{') || trimmed.starts_with("//") {
            continue;
        }

        let mut matched = false;

        if let Some(caps) = PASCAL_UNIT_RE.captures(trimmed) {
            push_class(
                &mut nodes, &mut edges, path, lineno, &file_id, &caps[1], trimmed,
            );
            matched = true;
        }
        if !matched {
            if let Some(caps) = PASCAL_PROGRAM_RE.captures(trimmed) {
                push_class(
                    &mut nodes, &mut edges, path, lineno, &file_id, &caps[1], trimmed,
                );
                matched = true;
            }
        }
        if !matched {
            if let Some(caps) = PASCAL_CLASS_RE.captures(trimmed) {
                push_class(
                    &mut nodes, &mut edges, path, lineno, &file_id, &caps[1], trimmed,
                );
                matched = true;
            }
        }
        if !matched {
            if let Some(caps) = PASCAL_ROUTINE_RE.captures(trimmed) {
                let name = caps[2].to_string();
                let label = name.rsplit('.').next().unwrap_or(&name).to_string();
                let id = make_node_id(&[&file_id, &name]);
                nodes.push(ExtractedNode {
                    id: id.clone(),
                    label: format!("{label}()"),
                    source_file: path.to_path_buf(),
                    source_line: Some(lineno),
                    docstring: None,
                    signature: Some(collapse(trimmed)),
                    node_type: "function".to_string(),
                });
                edges.push(ExtractedEdge {
                    source: file_id.clone(),
                    target: id,
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    confidence_score: None,
                    source_file: path.to_path_buf(),
                    source_line: Some(lineno),
                });
                matched = true;
            }
        }
        if !matched {
            if let Some(caps) = PASCAL_USES_RE.captures(trimmed) {
                for unit in caps[1].split(',') {
                    let unit = unit.trim();
                    if unit.is_empty() {
                        continue;
                    }
                    edges.push(ExtractedEdge {
                        source: file_id.clone(),
                        target: unit.to_string(),
                        relation: "imports".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        confidence_score: None,
                        source_file: path.to_path_buf(),
                        source_line: Some(lineno),
                    });
                }
            }
        }
    }

    Ok(Extraction {
        file_path: path.to_path_buf(),
        language: "Pascal/Delphi".to_string(),
        nodes,
        edges,
    })
}

fn push_class(
    nodes: &mut Vec<ExtractedNode>,
    edges: &mut Vec<ExtractedEdge>,
    path: &Path,
    lineno: u32,
    file_id: &str,
    name: &str,
    decl: &str,
) {
    let id = make_node_id(&[file_id, name]);
    nodes.push(ExtractedNode {
        id: id.clone(),
        label: name.to_string(),
        source_file: path.to_path_buf(),
        source_line: Some(lineno),
        docstring: None,
        signature: Some(collapse(decl)),
        node_type: "class".to_string(),
    });
    edges.push(ExtractedEdge {
        source: file_id.to_string(),
        target: id,
        relation: "contains".to_string(),
        confidence: "EXTRACTED".to_string(),
        confidence_score: None,
        source_file: path.to_path_buf(),
        source_line: Some(lineno),
    });
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
    fn pascal_unit_procedures_classes_and_uses() {
        let source = "unit Greeter;\r\n\r\ninterface\r\n\r\nuses SysUtils, Classes;\r\n\r\ntype\r\n  TGreeter = class(TObject)\r\n  public\r\n    function Greet(const name: string): string;\r\n  end;\r\n\r\nimplementation\r\n\r\nfunction TGreeter.Greet(const name: string): string;\r\nbegin\r\n  Result := 'Hi ' + name;\r\nend;\r\n\r\nprocedure Boot;\r\nbegin\r\nend;\r\n\r\nend.\r\n";
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Greeter.pas");
        fs::write(&file, source).unwrap();
        let db = open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        let ext = results.remove(0);

        assert_eq!(ext.language, "Pascal/Delphi");
        let labels: Vec<&str> = ext.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(labels.contains(&"Greeter"), "unit: {labels:?}");
        assert!(labels.contains(&"TGreeter"), "class: {labels:?}");
        assert!(labels.contains(&"Greet()"), "function: {labels:?}");
        assert!(labels.contains(&"Boot()"), "procedure: {labels:?}");
        assert!(ext.edges.iter().any(|e| e.relation == "imports"));
        assert!(ext
            .edges
            .iter()
            .any(|e| e.relation == "contains" && e.target.to_lowercase().ends_with("tgreeter")));
    }
}
