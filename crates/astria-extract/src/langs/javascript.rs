use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Javascript
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Javascript
            .registration()
            .extensions,
        #[cfg(feature = "lang-javascript")]
        language_fn: || tree_sitter_javascript::LANGUAGE.into(),
        #[cfg(not(feature = "lang-javascript"))]
        language_fn: || crate::langs::config::missing_language("javascript"),
        compiled_in: cfg!(feature = "lang-javascript"),
        class_types: &["class_declaration"],
        function_types: &[
            "function_declaration",
            "generator_function_declaration",
            "method_definition",
            "function_expression",
            "generator_function",
            "arrow_function",
        ],
        import_types: &["import_statement", "import_declaration"],
        call_type: "call_expression",
        name_child: None,
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["statement_block"],
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

/// Canonicalize statically named access without guessing dynamic receivers or
/// computed values. `api['run']` and `api.run` denote the same binding.
pub(crate) fn javascript_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" | "property_identifier" | "private_property_identifier" | "this" | "super" => {
            Some(node_text(&node, source).to_string())
        }
        "string" => Some(unquote_literal(node_text(&node, source)).to_string()),
        "member_expression" => Some(format!(
            "{}.{}",
            javascript_name(node.child_by_field_name("object")?, source)?,
            javascript_name(node.child_by_field_name("property")?, source)?
        )),
        "subscript_expression" => {
            let index = node.child_by_field_name("index")?;
            if index.kind() != "string" {
                return None;
            }
            Some(format!(
                "{}.{}",
                javascript_name(node.child_by_field_name("object")?, source)?,
                javascript_name(index, source)?
            ))
        }
        "computed_property_name" => {
            let key = node.named_child(0)?;
            (key.kind() == "string")
                .then(|| javascript_name(key, source))
                .flatten()
        }
        "parenthesized_expression" | "non_null_expression" => {
            javascript_name(node.named_child(0)?, source)
        }
        _ => None,
    }
}

/// Follow the value's binding, rather than an expression's private function
/// name. Object literals retain their containing binding (exports.api.run).
pub(crate) fn javascript_binding(node: Node<'_>, source: &[u8]) -> Option<String> {
    let parent = node.parent()?;
    let field = match parent.kind() {
        "variable_declarator" => "name",
        "assignment_expression" => "left",
        "pair" => "key",
        "public_field_definition" | "field_definition" => "name",
        "parenthesized_expression"
        | "as_expression"
        | "satisfies_expression"
        | "type_assertion"
        | "non_null_expression" => {
            return javascript_binding(parent, source);
        }
        _ => return None,
    };
    let value_field = if parent.kind() == "assignment_expression" {
        "right"
    } else {
        "value"
    };
    if parent
        .child_by_field_name(value_field)
        .is_none_or(|value| value.id() != node.id())
    {
        return None;
    }
    let key = parent.child_by_field_name(field)?;
    let name = javascript_name(key, source).unwrap_or_else(|| node_text(&key, source).to_string());
    if parent.kind() == "pair" {
        if let Some(prefix) = parent
            .parent()
            .and_then(|object| javascript_binding(object, source))
        {
            return Some(format!("{prefix}.{name}"));
        }
    }
    Some(name)
}

pub(crate) fn javascript_doc(node: Node<'_>, source: &[u8]) -> Option<String> {
    let mut anchor = node;
    loop {
        if let Some(comment) = anchor
            .prev_named_sibling()
            .filter(|n| n.kind() == "comment")
        {
            let raw = node_text(&comment, source);
            if raw.starts_with("/**") {
                return Some(
                    raw.trim_start_matches("/**")
                        .trim_end_matches("*/")
                        .lines()
                        .map(|line| line.trim().trim_start_matches('*').trim())
                        .collect::<Vec<_>>()
                        .join("\n")
                        .trim()
                        .to_string(),
                );
            }
        }
        let parent = anchor.parent()?;
        if !matches!(
            parent.kind(),
            "variable_declarator"
                | "lexical_declaration"
                | "variable_declaration"
                | "assignment_expression"
                | "expression_statement"
                | "pair"
                | "export_statement"
                | "parenthesized_expression"
                | "as_expression"
                | "satisfies_expression"
                | "public_field_definition"
                | "field_definition"
        ) {
            return None;
        }
        anchor = parent;
    }
}

pub(crate) fn walk_javascript_function<'a>(state: &mut ExtractionState<'a>, node: &Node<'a>) {
    let parent_id = state
        .lexical_scopes
        .last()
        .unwrap_or(&state.file_id)
        .clone();
    let name = javascript_binding(*node, state.source)
        .or_else(|| {
            node.child_by_field_name("name").map(|name| {
                let name = javascript_name(name, state.source)
                    .unwrap_or_else(|| node_text(&name, state.source).to_string());
                if node.kind() == "method_definition" {
                    if let Some(prefix) = node
                        .parent()
                        .filter(|p| p.kind() == "object")
                        .and_then(|object| javascript_binding(object, state.source))
                    {
                        return format!("{prefix}.{name}");
                    }
                }
                name
            })
        })
        .unwrap_or_else(|| {
            let count = state.closure_counts.entry(parent_id.clone()).or_insert(0);
            *count += 1;
            format!("{{closure#{count}}}")
        });
    let base_id = make_node_id(&[&parent_id, &name]);
    // Separate block-local bindings and repeated assignments in the same scope.
    let mut func_id = base_id.clone();
    let mut ordinal = 1;
    while state.nodes.iter().any(|existing| existing.id == func_id) {
        ordinal += 1;
        func_id = make_node_id(&[&base_id, &ordinal.to_string()]);
    }
    state.nodes.push(ExtractedNode {
        id: func_id.clone(),
        label: format!("{name}()"),
        source_file: state.file_path.clone(),
        source_line: Some(node.start_position().row as u32),
        docstring: javascript_doc(*node, state.source),
        signature: node_signature(node, state.source, state.cfg),
        node_type: "function".to_string(),
    });
    state.edges.push(ExtractedEdge {
        source: parent_id,
        target: func_id.clone(),
        relation: "contains".to_string(),
        confidence: "EXTRACTED".to_string(),
        confidence_score: Some(1.0),
        source_file: state.file_path.clone(),
        source_line: Some(node.start_position().row as u32),
    });
    if let Some(body) = find_body(node, state.cfg) {
        walk_calls(state, &func_id, &body);
    }
    state.lexical_scopes.push(func_id);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_structural(state, &child);
    }
    state.lexical_scopes.pop();
}
