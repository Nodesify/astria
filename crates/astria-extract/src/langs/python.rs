use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Python
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Python
            .registration()
            .extensions,
        #[cfg(feature = "lang-python")]
        language_fn: || tree_sitter_python::LANGUAGE.into(),
        #[cfg(not(feature = "lang-python"))]
        language_fn: || crate::langs::config::missing_language("python"),
        compiled_in: cfg!(feature = "lang-python"),
        class_types: &["class_definition"],
        function_types: &["function_definition"],
        import_types: &["import_statement", "import_from_statement"],
        call_type: "call",
        name_child: None,
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &[],
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

pub(crate) fn python_overload(node: Node<'_>, source: &[u8]) -> bool {
    let Some(decorated) = node.parent().filter(|p| p.kind() == "decorated_definition") else {
        return false;
    };
    let mut cursor = decorated.walk();
    let found = decorated.named_children(&mut cursor).any(|decorator| {
        if decorator.kind() != "decorator" {
            return false;
        }
        let Some(expression) = decorator.named_child(0) else {
            return false;
        };
        match expression.kind() {
            "identifier" => node_text(&expression, source) == "overload",
            "attribute" => expression
                .child_by_field_name("attribute")
                .is_some_and(|name| node_text(&name, source) == "overload"),
            _ => false,
        }
    });
    found
}

/// An overload is a declaration of the following runtime callable, not an
/// alternative body. Keep declarations only when no implementation exists
/// in their lexical block (as in a stub file).
pub(crate) fn python_overload_has_implementation(node: Node<'_>, source: &[u8]) -> bool {
    if !python_overload(node, source) {
        return false;
    }
    let Some(name) = node.child_by_field_name("name") else {
        return false;
    };
    let Some(block) = node.parent().and_then(|decorated| decorated.parent()) else {
        return false;
    };
    let mut cursor = block.walk();
    let found = block.named_children(&mut cursor).any(|sibling| {
        let definition = if sibling.kind() == "decorated_definition" {
            sibling.child_by_field_name("definition")
        } else {
            Some(sibling)
        };
        definition.is_some_and(|definition| {
            definition.kind() == "function_definition"
                && definition
                    .child_by_field_name("name")
                    .is_some_and(|other| node_text(&other, source) == node_text(&name, source))
                && !python_overload(definition, source)
        })
    });
    found
}
