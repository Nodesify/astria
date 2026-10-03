use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Rust.registration().name,
        extensions: astria_core::languages::LanguageId::Rust
            .registration()
            .extensions,
        #[cfg(feature = "lang-rust")]
        language_fn: || tree_sitter_rust::LANGUAGE.into(),
        #[cfg(not(feature = "lang-rust"))]
        language_fn: || crate::langs::config::missing_language("rust"),
        compiled_in: cfg!(feature = "lang-rust"),
        class_types: &["struct_item", "enum_item", "trait_item", "impl_item"],
        function_types: &["function_item", "function_signature_item"],
        import_types: &["use_declaration"],
        call_type: "call_expression",
        name_child: None,
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["block"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}

// --- Language-specific walker functions (moved from walkers.rs) ---
// These branches belong beside the language's config: each one encodes how
// THIS language's tree-sitter grammar names, documents, or classifies items.

#[allow(unused_imports)]
use crate::builtins::is_language_builtin;
#[allow(unused_imports)]
use crate::naming::{file_stem, make_node_id, make_target_id};
#[allow(unused_imports)]
use crate::schema::{ExtractedEdge, ExtractedNode, Extraction};
#[allow(unused_imports)]
use crate::walkers::ExtractionState;
#[allow(unused_imports)]
use crate::walkers::{
    const_signature, extract_docstring, find_body, first_child_text, item_docstring,
    node_signature, node_text, second_child, synthesize_closure_name, unquote_literal, walk_calls,
    walk_structural,
};
#[allow(unused_imports)]
use astria_core::AstriaError;
#[allow(unused_imports)]
use std::collections::{HashMap, HashSet};
#[allow(unused_imports)]
use tree_sitter::{Node, Parser};

/// Rust `///` doc comment block attached to the item above it. Rust doc
/// comments are sibling `line_comment`/`block_comment` nodes — unlike the
/// in-body docstrings `extract_docstring` handles — so they need their own
/// walk up the sibling chain. Non-doc comments stop the walk; `//!` module
/// comments are not item docs and never start it.
pub(crate) fn rust_doc_comment(node: &Node, source: &[u8]) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(comment) = current {
        let raw = node_text(&comment, source);
        let trimmed = raw.trim_start();
        let is_doc = trimmed.starts_with("///") || trimmed.starts_with("/**");
        if !is_doc {
            break;
        }
        let stripped = trimmed
            .trim_start_matches("///")
            .trim_start_matches("/**")
            .trim_end_matches("*/")
            .trim()
            .to_string();
        lines.push(stripped);
        current = comment.prev_sibling();
    }
    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    let joined = lines.join(" ").trim().to_string();
    (!joined.is_empty()).then_some(joined)
}

/// Rust `//!` module doc block: contiguous inner doc comments at the top of
/// the file, describing the module itself — the file node's own words for
/// "what is this file" questions. A leading plain comment (license header)
/// stops the block, so only a genuine module doc is captured.
pub(crate) fn rust_module_doc(root: &Node, source: &[u8]) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() != "line_comment" {
            break;
        }
        let raw = node_text(&child, source);
        let Some(doc) = raw.trim_start().strip_prefix("//!") else {
            break;
        };
        lines.push(doc.trim().to_string());
    }
    if lines.is_empty() {
        return None;
    }
    let joined = lines.join(" ").trim().to_string();
    (!joined.is_empty()).then_some(joined)
}

/// True when the item carries a visibility modifier (`pub`, `pub(crate)`,
/// ...). Prefers the grammar's `visibility` field, with a kind-scan
/// fallback for grammar versions without it.
pub(crate) fn rust_is_pub(node: &Node) -> bool {
    if node.child_by_field_name("visibility").is_some() {
        return true;
    }
    let mut cursor = node.walk();
    let any = node
        .children(&mut cursor)
        .any(|child| child.kind() == "visibility_modifier");
    any
}

// ---------------------------------------------------------------------------
// Pass 1: Structural extraction + inline call-graph
// ---------------------------------------------------------------------------

/// Rust attributes are sibling AST nodes, not part of a function's text.
/// Inspect only attached attributes and enclosing items so strings/comments
/// mentioning tests cannot classify production functions as tests.
pub(crate) fn is_rust_test_function(node: Node<'_>, source: &[u8]) -> bool {
    let mut current = Some(node);
    while let Some(item) = current {
        if item.kind() == "mod_item"
            && item
                .child_by_field_name("name")
                .is_some_and(|name| node_text(&name, source) == "tests")
        {
            return true;
        }
        let mut previous = item.prev_named_sibling();
        while let Some(attribute) = previous {
            match attribute.kind() {
                "attribute_item" => {
                    let text: String = node_text(&attribute, source)
                        .chars()
                        .filter(|ch| !ch.is_whitespace())
                        .collect();
                    let content = text.strip_prefix("#[").and_then(|s| s.strip_suffix(']'));
                    if content.is_some_and(|content| {
                        content == "cfg(test)"
                            || ["test", "tokio::test", "async_std::test"]
                                .iter()
                                .any(|name| {
                                    content == *name
                                        || content
                                            .strip_prefix(name)
                                            .is_some_and(|suffix| suffix.starts_with('('))
                                })
                    }) {
                        return true;
                    }
                }
                "line_comment" | "block_comment" => {}
                _ => break,
            }
            previous = attribute.prev_named_sibling();
        }
        current = item.parent();
    }
    false
}
