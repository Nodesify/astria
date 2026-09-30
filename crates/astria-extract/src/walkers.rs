// walkers: tree-sitter AST extraction — structural pass (imports, classes,
// functions), inline call-graph pass, rationale comments, and the
// single-file driver that ties them together.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

use crate::builtins::is_language_builtin;
use crate::langs::LanguageConfig;
use crate::naming::{file_stem, make_node_id, make_target_id};
use crate::schema::{ExtractedEdge, ExtractedNode, Extraction};
use astria_core::AstriaError;

// ---------------------------------------------------------------------------
// AST text helpers
// ---------------------------------------------------------------------------

/// Extract UTF-8 text from a source byte slice for the given node.
pub(crate) fn node_text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    let range = node.byte_range();
    std::str::from_utf8(&source[range]).unwrap_or("")
}

/// Get the text of the first child of a node. Returns None if no children.
fn first_child_text<'a>(node: &Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    let child = node.children(&mut cursor).next()?;
    Some(node_text(&child, source))
}

/// Get the second child of a tree-sitter node.
/// The nth (1-based) named child: positional naming for grammars without
/// named fields (HCL blocks name themselves from their label string, which
/// is the 2nd named child).
fn nth_named_child<'a>(node: &Node<'a>, n: usize) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let named: Vec<_> = node
        .children(&mut cursor)
        .filter(|c| c.is_named())
        .collect();
    named.get(n.saturating_sub(1)).copied()
}

fn second_child<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let child = node.children(&mut cursor).nth(1);
    drop(cursor);
    child
}

/// Find the body node using body_field first, then falling back to child types.
#[allow(clippy::manual_find)]
fn find_body<'a>(node: &Node<'a>, cfg: &LanguageConfig) -> Option<Node<'a>> {
    if let Some(field) = cfg.body_field {
        if let Some(body) = node.child_by_field_name(field) {
            return Some(body);
        }
    }
    // Fallback: look for a child whose kind is in body_fallback_types
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if cfg.body_fallback_types.contains(&child.kind()) {
            return Some(child);
        }
    }
    None
}

/// Signature line(s): source text from the item start to the start of its
/// body, whitespace-collapsed and capped. Lets agents see WHAT a symbol is
/// without opening the file.
fn node_signature(node: &Node, source: &[u8], cfg: &LanguageConfig) -> Option<String> {
    let body_start = find_body(node, cfg)
        .map(|b| b.byte_range().start)
        .unwrap_or_else(|| node.byte_range().end);
    let start = node.byte_range().start;
    if body_start <= start {
        return None;
    }
    let raw = std::str::from_utf8(&source[start..body_start.min(source.len())]).unwrap_or("");
    let mut sig = String::new();
    for word in raw.split_whitespace() {
        sig.push_str(word);
        sig.push(' ');
        if sig.len() > 200 {
            break;
        }
    }
    let sig = sig.trim_end().to_string();
    if sig.is_empty() {
        None
    } else {
        Some(sig)
    }
}

/// Extract the docstring: the first string/expression in the body.
fn extract_docstring(node: &Node, source: &[u8], cfg: &LanguageConfig) -> Option<String> {
    let body = find_body(node, cfg)?;
    let mut cursor = body.walk();
    if let Some(child) = body.children(&mut cursor).next() {
        let kind = child.kind();
        if kind == "string" || kind == "string_literal" || kind == "expression_statement" {
            let text = node_text(&child, source);
            // Strip quotes
            let cleaned = text
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .trim_start_matches("\"\"\"")
                .trim_end_matches("\"\"\"")
                .trim_start_matches("'''")
                .trim_end_matches("'''")
                .trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

/// Rust `///` doc comment block attached to the item above it. Rust doc
/// comments are sibling `line_comment`/`block_comment` nodes — unlike the
/// in-body docstrings `extract_docstring` handles — so they need their own
/// walk up the sibling chain. Non-doc comments stop the walk; `//!` module
/// comments are not item docs and never start it.
fn rust_doc_comment(node: &Node, source: &[u8]) -> Option<String> {
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

/// An item's docstring in the language's own convention: Rust documents
/// above the item (`///`), the others document inside the body.
fn item_docstring(state: &ExtractionState, node: &Node) -> Option<String> {
    if state.cfg.name == "Rust" {
        rust_doc_comment(node, state.source)
    } else {
        extract_docstring(node, state.source, state.cfg)
    }
}

/// Rust `//!` module doc block: contiguous inner doc comments at the top of
/// the file, describing the module itself — the file node's own words for
/// "what is this file" questions. A leading plain comment (license header)
/// stops the block, so only a genuine module doc is captured.
fn rust_module_doc(root: &Node, source: &[u8]) -> Option<String> {
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

/// Whitespace-collapsed declaration text of a const/static item, capped
/// like `node_signature`. There is no body to cut at — the initializer IS
/// the answer content ("pub const MODEL: ... = JinaEmbeddingsV2BaseCode;")
/// for value questions no function node can carry.
fn const_signature(node: &Node, source: &[u8]) -> Option<String> {
    let raw = node_text(node, source);
    let mut sig = String::new();
    for word in raw.split_whitespace() {
        if sig.len() + word.len() + 1 > 200 {
            break;
        }
        if !sig.is_empty() {
            sig.push(' ');
        }
        sig.push_str(word);
    }
    let sig = sig.trim_end().to_string();
    (!sig.is_empty()).then_some(sig)
}

/// True when the item carries a visibility modifier (`pub`, `pub(crate)`,
/// ...). Prefers the grammar's `visibility` field, with a kind-scan
/// fallback for grammar versions without it.
fn rust_is_pub(node: &Node) -> bool {
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

pub(crate) struct ExtractionState<'a> {
    pub cfg: &'a LanguageConfig,
    pub source: &'a [u8],
    pub file_id: String,
    pub file_path: PathBuf,
    pub nodes: Vec<ExtractedNode>,
    pub edges: Vec<ExtractedEdge>,
    pub current_class_id: Option<String>,
    /// Display label of the enclosing class, for scope-qualified closure
    /// labels (`PriceCalc::{closure#1}()`) that stay unique corpus-wide so
    /// the build's fuzzy label-dedup cannot merge two different closures.
    pub current_class_label: Option<String>,
    /// Identifier-shaped string literals already indexed for this file,
    /// keyed by lowercased literal (one reference node per distinct literal
    /// per file; the node itself is global across files).
    pub string_refs_seen: HashSet<String>,
    /// Anonymous-closure ordinals per scope (class id or file id). Ordinals
    /// are stable under line edits — only reordering or inserting a closure
    /// earlier in the same scope shifts later ones.
    pub closure_counts: HashMap<String, usize>,
    /// JavaScript lexical function/class nesting, independent of class context.
    pub lexical_scopes: Vec<String>,
}

fn is_javascript(cfg: &LanguageConfig) -> bool {
    matches!(cfg.name, "JavaScript" | "TypeScript")
}

fn python_overload(node: Node<'_>, source: &[u8]) -> bool {
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
fn python_overload_has_implementation(node: Node<'_>, source: &[u8]) -> bool {
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

/// Rust attributes are sibling AST nodes, not part of a function's text.
/// Inspect only attached attributes and enclosing items so strings/comments
/// mentioning tests cannot classify production functions as tests.
fn is_rust_test_function(node: Node<'_>, source: &[u8]) -> bool {
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

/// Canonicalize statically named access without guessing dynamic receivers or
/// computed values. `api['run']` and `api.run` denote the same binding.
fn javascript_name(node: Node<'_>, source: &[u8]) -> Option<String> {
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
fn javascript_binding(node: Node<'_>, source: &[u8]) -> Option<String> {
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

fn javascript_doc(node: Node<'_>, source: &[u8]) -> Option<String> {
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

fn walk_javascript_function<'a>(state: &mut ExtractionState<'a>, node: &Node<'a>) {
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

/// Maximum reference nodes extracted from one file — bounds the graph cost
/// of string-literal indexing.
const MAX_STRING_REFS_PER_FILE: usize = 40;

/// True when a string literal is identifier-shaped enough to index as a
/// reference: env-var style (`PLANE_URL`, `NODE_ENV`), snake_case keys
/// (`needs_human`), and dotted/kebab/slash chains (`cli.command`,
/// `harness/hr-101-fix-redis-leak`). Plain words ("retry", "error") are
/// deliberately excluded — they are prose/UI text far more often than
/// shared references, and would drown the graph in noise.
fn is_reference_literal(s: &str) -> bool {
    if s.is_empty() || s.len() < 3 || s.len() > 64 {
        return false;
    }
    let bytes = s.as_bytes();
    if bytes
        .iter()
        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
        && s.contains('_')
    {
        return true;
    }
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    let mut separators = 0;
    for &b in bytes {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => {}
            b'_' | b'.' | b'-' | b'/' => separators += 1,
            _ => return false,
        }
    }
    separators >= 1
}

/// Strip a string literal's raw source text down to its content: remove
/// surrounding quotes/backticks and language prefixes (`r#"..."`, `b".."`).
fn unquote_literal(raw: &str) -> &str {
    let t = raw.trim();
    let body = if t.len() > 2 && t.as_bytes()[0].is_ascii_alphabetic() {
        &t[1..]
    } else {
        t
    };
    let bytes = body.as_bytes();
    let quoted = bytes.len() >= 2
        && matches!(
            (bytes.first().copied(), bytes.last().copied()),
            (Some(b'"'), Some(b'"')) | (Some(b'\''), Some(b'\'')) | (Some(b'`'), Some(b'`'))
        );
    if quoted {
        &body[1..body.len() - 1]
    } else {
        body
    }
}

/// Collect an identifier-shaped string literal as a global reference node
/// (`str::<literal>` id) plus a `references` edge from this file's node.
fn collect_string_refs(state: &mut ExtractionState<'_>, node: &Node<'_>) {
    if state.string_refs_seen.len() >= MAX_STRING_REFS_PER_FILE {
        return;
    }
    let literal = unquote_literal(node_text(node, state.source));
    if !is_reference_literal(literal) {
        return;
    }
    if !state.string_refs_seen.insert(literal.to_lowercase()) {
        return;
    }
    let line = node.start_position().row as u32;
    let ref_id = make_node_id(&["str", literal.to_lowercase().as_str()]);
    state.nodes.push(ExtractedNode {
        id: ref_id.clone(),
        label: literal.to_string(),
        source_file: state.file_path.clone(),
        source_line: Some(line),
        docstring: None,
        signature: None,
        node_type: "reference".to_string(),
    });
    state.edges.push(ExtractedEdge {
        source: state.file_id.clone(),
        target: ref_id,
        relation: "references".to_string(),
        confidence: "EXTRACTED".to_string(),
        confidence_score: Some(1.0),
        source_file: state.file_path.clone(),
        source_line: Some(line),
    });
}

// ---------------------------------------------------------------------------
// Anonymous closures (PHP): synthesized names and attribution boundaries
// ---------------------------------------------------------------------------

/// PHP routing verbs whose first string argument is a route path. A closure
/// passed directly to one of these gets a `VERB /path` label instead of an
/// ordinal (#3409).
const PHP_ROUTING_VERBS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "options", "any", "match", "map",
];

/// First string literal among `arg_list`'s children, stopping at `before`
/// (the argument the closure itself sits in) when given. Handles both bare
/// string children and `argument`-wrapped strings.
fn first_string_arg<'a>(
    arg_list: &Node<'a>,
    before: Option<&Node<'_>>,
    source: &'a [u8],
) -> Option<String> {
    let mut cursor = arg_list.walk();
    for child in arg_list.children(&mut cursor) {
        if before.is_some_and(|b| child.id() == b.id()) {
            break;
        }
        let target = if child.kind() == "argument" {
            let mut c = child.walk();
            let found = child
                .children(&mut c)
                .find(|g| g.kind() == "string" || g.kind() == "encapsed_string");
            drop(c);
            found
        } else if child.kind() == "string" || child.kind() == "encapsed_string" {
            Some(child)
        } else {
            None
        };
        if let Some(t) = target {
            return Some(unquote_literal(node_text(&t, source)).to_string());
        }
    }
    None
}

/// Route-derived name for a PHP closure passed to a routing call: walk up the
/// AST collecting the innermost route's verb/path plus any enclosing
/// `group()`/`prefix()` path arguments and fluent-chain prefixes, composing
/// e.g. `GET /api/v1/users/{id}`. Returns None when the innermost enclosing
/// call is not a route (so `$cache->get('key', fn)` stays an ordinal).
fn php_route_name(closure: &Node<'_>, source: &[u8]) -> Option<String> {
    let mut prefixes: Vec<String> = Vec::new();
    let mut verb: Option<String> = None;
    let mut curr = closure.parent();

    while let Some(n) = curr {
        let kind = n.kind();
        if matches!(
            kind,
            "function_definition" | "method_declaration" | "class_declaration"
        ) {
            break;
        }
        if kind == "argument" {
            let arg_list = n.parent().filter(|a| a.kind() == "arguments");
            let call = arg_list.and_then(|a| a.parent());
            if let Some(call) = call.filter(|c| {
                matches!(
                    c.kind(),
                    "member_call_expression"
                        | "function_call_expression"
                        | "scoped_call_expression"
                )
            }) {
                let name_node = call
                    .child_by_field_name("name")
                    .or_else(|| call.child_by_field_name("function"));
                let raw_method = name_node
                    .map(|m| node_text(&m, source))
                    .unwrap_or("")
                    .to_lowercase();
                let path_text = first_string_arg(&arg_list.unwrap(), Some(&n), source);
                if verb.is_none() {
                    // The innermost call must be a routing verb whose path
                    // starts with '/', or this closure is not a route.
                    match path_text {
                        Some(p)
                            if p.starts_with('/')
                                && PHP_ROUTING_VERBS.contains(&raw_method.as_str()) =>
                        {
                            verb = Some(raw_method.to_uppercase());
                            prefixes.push(p);
                        }
                        _ => return None,
                    }
                } else if let Some(p) = path_text.filter(|p| !p.is_empty()) {
                    // Outer calls (group()/prefix()) contribute path prefixes.
                    prefixes.push(if p.starts_with('/') {
                        p
                    } else {
                        format!("/{}", p)
                    });
                }

                // Fluent chain prefixes on the same statement
                // (Route::prefix('/x')->group(...)).
                let mut fluent = call.child_by_field_name("object");
                while let Some(f) = fluent.filter(|f| f.kind() == "member_call_expression") {
                    if let Some(f_args) = f.child_by_field_name("arguments") {
                        if let Some(p) = first_string_arg(&f_args, None, source) {
                            if !p.is_empty() {
                                prefixes.push(if p.starts_with('/') {
                                    p
                                } else {
                                    format!("/{}", p)
                                });
                            }
                        }
                    }
                    fluent = f.child_by_field_name("object");
                }

                curr = call.parent();
                continue;
            }
        } else if kind == "anonymous_function" || kind == "arrow_function" {
            // A non-route closure nested inside another closure never adopts
            // the outer one's route; with a route already found, jump across
            // the closure boundary to its own argument.
            verb.as_ref()?;
            curr = n.parent();
            continue;
        }
        curr = n.parent();
    }

    let verb = verb?;
    // Prefixes are collected inside-out; join outermost-first under '/'.
    let full = format!(
        "/{}",
        prefixes
            .iter()
            .rev()
            .map(|p| p.trim_matches('/'))
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    );
    Some(format!("{} {}", verb, full))
}

/// Synthesized name for an anonymous closure node: route label when PHP
/// routing detection matches, else a stable per-scope ordinal. The ordinal
/// label carries its scope (`PriceCalc::{closure#1}()`, or
/// `<file-stem>::{closure#1}()` at file scope) so two closures never share a
/// label — the build's fuzzy dedup merges same-label nodes and would
/// otherwise misattribute one closure's call edges onto the other.
fn synthesize_closure_name(state: &mut ExtractionState<'_>, node: &Node<'_>) -> String {
    if state.cfg.name == "PHP" {
        if let Some(route) = php_route_name(node, state.source) {
            return route;
        }
    }
    // ponytail: class-scope labels use the bare class label, so two
    // same-named classes can still collide; qualify by class id if that
    // shows up in practice.
    let (scope_key, scope_display) = match (&state.current_class_id, &state.current_class_label) {
        (Some(id), Some(label)) => (id.clone(), label.clone()),
        (Some(id), None) => (id.clone(), id.clone()),
        (None, _) => (state.file_id.clone(), state.file_id.clone()),
    };
    let count = state.closure_counts.entry(scope_key).or_insert(0);
    *count += 1;
    format!("{}::{{closure#{}}}", scope_display, count)
}

pub(crate) fn walk_structural<'a>(state: &mut ExtractionState<'a>, node: &Node<'a>) {
    let kind = node.kind();

    // --- Identifier-shaped string literals ---
    // Kinds whose name ends in "string" are literals in every grammar we
    // support (string, string_literal, interpreted_string_literal,
    // template_string, ...) while type annotations ("string_type") are
    // excluded by the suffix rule. Literals that look like identifiers
    // (env vars, snake/dotted/kebab/slash keys) become global reference
    // nodes so agents can trace where a config key or status value is used.
    if kind.ends_with("string") {
        collect_string_refs(state, node);
        return;
    }

    // --- Imports ---
    if state.cfg.import_types.contains(&kind) {
        // Call-name filter: if import_call_names is non-empty, check first child text
        let passes_filter = if state.cfg.import_call_names.is_empty() {
            true
        } else {
            first_child_text(node, state.source)
                .map(|t| state.cfg.import_call_names.contains(&t))
                .unwrap_or(false)
        };

        if passes_filter {
            let import_text = node_text(node, state.source);
            let module_name = extract_import_module(import_text, kind, state.cfg.name);
            if let Some(mod_name) = module_name {
                let target_id = make_target_id(&mod_name);
                state.edges.push(ExtractedEdge {
                    source: state.file_id.clone(),
                    target: target_id,
                    relation: "imports".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    confidence_score: Some(1.0),
                    source_file: state.file_path.clone(),
                    source_line: Some(node.start_position().row as u32),
                });
            }
        }
        // Still walk children for nested structures
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk_structural(state, &child);
        }
        return;
    }

    // --- Classes / structs / enums ---
    if state.cfg.class_types.contains(&kind) {
        // Call-name filter: if class_call_names is non-empty, check first child text
        let passes_filter = if state.cfg.class_call_names.is_empty() {
            true
        } else {
            first_child_text(node, state.source)
                .map(|t| state.cfg.class_call_names.contains(&t))
                .unwrap_or(false)
        };

        if passes_filter {
            // Try name_field first, then fall back to second child for call-based languages.
            // Containers without a name field (e.g. Rust `impl_item`, whose fields are
            // "trait"/"type") fall back to their `type` field text, so methods scope under
            // the impl type instead of colliding at file level when several impls
            // define the same method name.
            let type_field_node = node.child_by_field_name("type");
            let positional = state.cfg.name_child.and_then(|n| nth_named_child(node, n));
            let name_node = node
                .child_by_field_name(state.cfg.name_field)
                .or(positional)
                .or(type_field_node);
            let name_node = match name_node {
                Some(n) => Some(n),
                None if !state.cfg.class_call_names.is_empty() => second_child(node),
                _ => None,
            };
            // A container named via the `type` fallback (an impl block) is a
            // scope boundary only: the real type node already exists (or will
            // come from the struct/enum declaration), so emitting one would
            // duplicate its id.
            let scope_only = type_field_node.is_some()
                && node.child_by_field_name(state.cfg.name_field).is_none()
                && kind == "impl_item";
            if let Some(name_node) = name_node {
                let name = node_text(&name_node, state.source).to_string();
                let parent_id = if is_javascript(state.cfg) {
                    state.lexical_scopes.last().unwrap_or(&state.file_id)
                } else {
                    &state.file_id
                }
                .clone();
                let class_id = make_node_id(&[&parent_id, &name]);
                let docstring = item_docstring(state, node);

                if !scope_only {
                    state.nodes.push(ExtractedNode {
                        id: class_id.clone(),
                        label: name.clone(),
                        source_file: state.file_path.clone(),
                        source_line: Some(node.start_position().row as u32),
                        docstring,
                        signature: node_signature(node, state.source, state.cfg),
                        node_type: "class".to_string(),
                    });

                    state.edges.push(ExtractedEdge {
                        source: parent_id,
                        target: class_id.clone(),
                        relation: "contains".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        confidence_score: Some(1.0),
                        source_file: state.file_path.clone(),
                        source_line: Some(node.start_position().row as u32),
                    });
                }

                // Walk children inside this class context
                if is_javascript(state.cfg) {
                    state.lexical_scopes.push(class_id.clone());
                }
                let prev_class = state.current_class_id.replace(class_id);
                let prev_label = state.current_class_label.replace(name.clone());
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    walk_structural(state, &child);
                }
                state.current_class_id = prev_class;
                state.current_class_label = prev_label;
                if is_javascript(state.cfg) {
                    state.lexical_scopes.pop();
                }
                return;
            }
        }
    }

    // --- Anonymous closures (closure_types): route names / ordinals ---
    // These are function-attribution boundaries: calls inside attribute to
    // the closure itself, so walk_calls on the enclosing function skips them.
    if state.cfg.closure_types.contains(&kind) {
        let name = synthesize_closure_name(state, node);
        let parent_id = state
            .current_class_id
            .clone()
            .unwrap_or_else(|| state.file_id.clone());
        let func_id = make_node_id(&[&parent_id, &name]);

        state.nodes.push(ExtractedNode {
            id: func_id.clone(),
            label: format!("{}()", name),
            source_file: state.file_path.clone(),
            source_line: Some(node.start_position().row as u32),
            docstring: extract_docstring(node, state.source, state.cfg),
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

        // Pass 2 inline: calls inside attribute to the closure.
        if let Some(body) = find_body(node, state.cfg) {
            walk_calls(state, &func_id, &body);
        }

        // Walk children for nested closures.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk_structural(state, &child);
        }
        return;
    }

    // --- Rust constants / statics ---
    // A `pub` (or `///`-documented) const/static is a graph citizen: value
    // questions ("which embedding model", "what threshold") are answered by
    // the constant's initializer, which no function node can carry. Private
    // undocumented constants stay out — they are implementation detail, and
    // the graph measured a real miss from their absence, not their noise.
    // Rust-only: that is where the gap was observed.
    if state.cfg.name == "Rust" && matches!(kind, "const_item" | "static_item") {
        let docstring = rust_doc_comment(node, state.source);
        if let Some(name_node) = node.child_by_field_name("name") {
            if rust_is_pub(node) || docstring.is_some() {
                let name = node_text(&name_node, state.source).to_string();
                let parent_id = state
                    .current_class_id
                    .clone()
                    .unwrap_or_else(|| state.file_id.clone());
                let const_id = make_node_id(&[&parent_id, &name]);

                state.nodes.push(ExtractedNode {
                    id: const_id.clone(),
                    label: name.clone(),
                    source_file: state.file_path.clone(),
                    source_line: Some(node.start_position().row as u32),
                    docstring: docstring.clone(),
                    signature: const_signature(node, state.source),
                    node_type: "constant".to_string(),
                });

                state.edges.push(ExtractedEdge {
                    source: parent_id,
                    target: const_id.clone(),
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    confidence_score: Some(1.0),
                    source_file: state.file_path.clone(),
                    source_line: Some(node.start_position().row as u32),
                });

                // A const initializer can call functions (`Size::MAX`, const
                // fns); attribute those calls to the constant itself.
                if let Some(value) = node.child_by_field_name("value") {
                    walk_calls(state, &const_id, &value);
                }
            }
        }

        // Walk children for nested structure (string refs in the value).
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk_structural(state, &child);
        }
        return;
    }

    // --- Functions / methods ---
    if state.cfg.function_types.contains(&kind) {
        if state.cfg.name == "Python" && python_overload_has_implementation(*node, state.source) {
            return;
        }
        if is_javascript(state.cfg) {
            walk_javascript_function(state, node);
            return;
        }
        // Call-name filter: if function_call_names is non-empty, check first child text
        let passes_filter = if state.cfg.function_call_names.is_empty() {
            true
        } else {
            first_child_text(node, state.source)
                .map(|t| state.cfg.function_call_names.contains(&t))
                .unwrap_or(false)
        };

        if passes_filter {
            // Try name_field first, then a positional named child, then fall
            // back to second child for call-based languages
            let positional = state.cfg.name_child.and_then(|n| nth_named_child(node, n));
            let name_node = node
                .child_by_field_name(state.cfg.name_field)
                .or(positional)
                .or_else(|| {
                    if !state.cfg.function_call_names.is_empty() {
                        second_child(node)
                    } else {
                        None
                    }
                });
            if let Some(name_node) = name_node {
                let name = node_text(&name_node, state.source).to_string();
                let func_label = format!("{}()", name);
                let parent_id = state.current_class_id.as_deref().unwrap_or(&state.file_id);
                let func_id = make_node_id(&[parent_id, &name]);

                let docstring = item_docstring(state, node);

                state.nodes.push(ExtractedNode {
                    id: func_id.clone(),
                    label: func_label,
                    source_file: state.file_path.clone(),
                    source_line: Some(node.start_position().row as u32),
                    docstring,
                    signature: node_signature(node, state.source, state.cfg),
                    node_type: if state.cfg.name == "Rust"
                        && is_rust_test_function(*node, state.source)
                    {
                        "test"
                    } else {
                        "function"
                    }
                    .to_string(),
                });

                state.edges.push(ExtractedEdge {
                    source: parent_id.to_string(),
                    target: func_id.clone(),
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    confidence_score: Some(1.0),
                    source_file: state.file_path.clone(),
                    source_line: Some(node.start_position().row as u32),
                });

                // Pass 2 inline: walk function body for call expressions
                if let Some(body) = find_body(node, state.cfg) {
                    walk_calls(state, &func_id, &body);
                }
            }
        }

        // Walk children for nested functions
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk_structural(state, &child);
        }
        return;
    }

    // Default: recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_structural(state, &child);
    }
}

/// Best-effort module name extraction from import text.
/// Dispatches on language first to avoid conflicts between languages
/// that share the same tree-sitter node kinds (e.g. "import_statement"
/// is used by both Python and JavaScript).
fn extract_import_module(text: &str, kind: &str, language: &str) -> Option<String> {
    match (language, kind) {
        ("Python", "import_statement" | "import_from_statement") => {
            let cleaned = text
                .trim()
                .trim_start_matches("import ")
                .trim_start_matches("from ");
            let first = cleaned.split_whitespace().next()?;
            let module = first.split('.').next()?;
            Some(module.to_string())
        }
        ("JavaScript" | "TypeScript", "import_statement" | "import_declaration") => {
            if let Some(pos) = text.find("from") {
                let after_from = &text[pos + 4..];
                let trimmed = after_from.trim();
                let module = trimmed
                    .trim_start_matches('"')
                    .trim_start_matches('\'')
                    .trim_start_matches('`')
                    .split(&['"', '\'', '`'][..])
                    .next()
                    .unwrap_or("");
                if !module.is_empty() {
                    return Some(module.to_string());
                }
            }
            // Require-style: require('module')
            if let Some(pos) = text.find("require(") {
                let after = &text[pos + 8..];
                let module = after
                    .trim_start_matches('"')
                    .trim_start_matches('\'')
                    .split(&['"', '\'', ')'][..])
                    .next()
                    .unwrap_or("");
                if !module.is_empty() {
                    return Some(module.to_string());
                }
            }
            None
        }
        ("Rust", "use_declaration") => {
            let cleaned = text.trim_start_matches("use").trim().trim_end_matches(';');
            let first = cleaned.split("::").next()?.trim();
            if !first.is_empty() {
                return Some(first.to_string());
            }
            None
        }
        ("Go", "import_declaration") => {
            let cleaned = text.trim_start_matches("import").trim();
            let module = cleaned
                .trim_start_matches('"')
                .split(&['"', '\n'][..])
                .next()
                .unwrap_or("");
            if !module.is_empty() {
                return Some(module.to_string());
            }
            None
        }
        ("Java" | "Scala", "import_declaration") => {
            let cleaned = text
                .trim_start_matches("import")
                .trim_start_matches("static")
                .trim()
                .trim_end_matches(';');
            let parts: Vec<&str> = cleaned.split('.').collect();
            if !parts.is_empty() {
                return Some(parts.join("."));
            }
            None
        }
        ("Swift", "import_declaration") => {
            let cleaned = text.trim_start_matches("import").trim();
            let module = cleaned.split_whitespace().next()?;
            if !module.is_empty() {
                return Some(module.to_string());
            }
            None
        }
        ("C" | "C++", "preproc_include") => {
            let cleaned = text.trim_start_matches("#include").trim();
            let module = cleaned
                .trim_start_matches('<')
                .trim_start_matches('"')
                .split(&['>', '"'][..])
                .next()
                .unwrap_or("");
            if !module.is_empty() {
                return Some(module.to_string());
            }
            None
        }
        ("CSS", "import_statement") => {
            let cleaned = text.trim_start_matches("@import").trim();
            let module = cleaned
                .trim_start_matches('"')
                .trim_start_matches('\'')
                .trim_start_matches("url(")
                .trim_start_matches('"')
                .trim_start_matches('\'')
                .split(&['"', '\'', ')'][..])
                .next()
                .unwrap_or("");
            if !module.is_empty() {
                return Some(module.to_string());
            }
            None
        }
        ("Elixir", "call") => {
            // Elixir imports: use MyModule, import MyModule, alias My.Module, require MyModule
            let cleaned = text.trim();
            let keyword = cleaned.split_whitespace().next()?;
            if !matches!(keyword, "use" | "import" | "alias" | "require") {
                return None;
            }
            let after_keyword = cleaned.trim_start_matches(keyword).trim();
            let module = after_keyword.split(&[' ', ',', '.'][..]).next()?;
            if !module.is_empty() {
                return Some(module.to_string());
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Pass 2: Call-graph extraction (walked inline during pass 1)
// ---------------------------------------------------------------------------

fn walk_calls<'a>(state: &mut ExtractionState<'a>, caller_id: &str, body: &Node<'a>) {
    let kind = body.kind();
    // An expression-bodied arrow can return another function directly.
    // That returned function owns its calls, just like a nested declaration.
    if is_javascript(state.cfg)
        && (state.cfg.function_types.contains(&kind) || state.cfg.class_types.contains(&kind))
    {
        return;
    }

    if kind == state.cfg.call_type {
        let callee_name = if is_javascript(state.cfg) {
            body.child_by_field_name("function")
                .and_then(|callee| javascript_name(callee, state.source))
        } else {
            extract_callee_name(body, state.source)
        };
        if let Some(name) = callee_name {
            // Skip language builtins: they resolve to bare-name stubs that
            // merge corpus-wide and pollute god-node rankings.
            if !is_language_builtin(&name, state.cfg.name) {
                let callee_id = make_target_id(&name);
                state.edges.push(ExtractedEdge {
                    source: caller_id.to_string(),
                    target: callee_id,
                    relation: "calls".to_string(),
                    confidence: "INFERRED".to_string(),
                    confidence_score: Some(0.7),
                    source_file: state.file_path.clone(),
                    source_line: Some(body.start_position().row as u32),
                });
            }
        }
    }

    // Recurse into children. Closures are attribution boundaries: their inner
    // calls are attributed by walk_structural when it names the closure, so
    // they must not also attribute to the enclosing function.
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if state.cfg.closure_types.contains(&child.kind())
            || (is_javascript(state.cfg)
                && (state.cfg.function_types.contains(&child.kind())
                    || state.cfg.class_types.contains(&child.kind())))
        {
            continue;
        }
        walk_calls(state, caller_id, &child);
    }
}

/// Extract the callee name from a call expression.
fn extract_callee_name(call_node: &Node, source: &[u8]) -> Option<String> {
    // The first child (field "function") is the callee
    let mut cursor = call_node.walk();
    let func_child = call_node.children(&mut cursor).next()?;

    let text = node_text(&func_child, source);
    // For method calls like obj.method(), take the last part
    let name = if text.contains('.') {
        text.split('.').next_back().unwrap_or(text)
    } else {
        text
    };
    Some(name.to_string())
}

// ---------------------------------------------------------------------------
// Rationale comment extraction
// ---------------------------------------------------------------------------

const RATIONALE_TAGS: &[&str] = &["NOTE", "WHY", "HACK", "IMPORTANT", "TODO", "FIXME"];

fn extract_rationale(state: &mut ExtractionState, source: &[u8]) {
    let comment_prefix = if state.cfg.name == "Python" {
        "#"
    } else {
        "//"
    };
    let text = match std::str::from_utf8(source) {
        Ok(t) => t,
        Err(_) => return,
    };

    // Build a sorted list of (line, node_id) to find nearest parent above each rationale
    let mut nodes_by_line: Vec<(u32, String)> = state
        .nodes
        .iter()
        .filter_map(|n| n.source_line.map(|l| (l, n.id.clone())))
        .collect();
    nodes_by_line.sort_by_key(|(l, _)| *l);

    for (lineno, line_text) in text.lines().enumerate() {
        let stripped = line_text.trim();
        let tag = match find_rationale_tag(stripped, comment_prefix) {
            Some(t) => t,
            None => continue,
        };

        let comment_text = stripped
            .trim_start_matches(comment_prefix)
            .trim_start_matches(&format!("{}:", tag))
            .trim();

        if comment_text.is_empty() {
            continue;
        }

        let line_num = lineno as u32 + 1;
        let rid = make_node_id(&[&state.file_id, "rationale", &line_num.to_string()]);

        // Find nearest parent node above this line
        let parent_id = nodes_by_line
            .iter()
            .rev()
            .find(|(l, _)| *l < line_num)
            .map(|(_, id)| id.clone())
            .unwrap_or_else(|| state.file_id.clone());

        let label = if comment_text.len() > 80 {
            format!("{}: {}", tag, &comment_text[..80])
        } else {
            format!("{}: {}", tag, comment_text)
        };

        state.nodes.push(ExtractedNode {
            id: rid.clone(),
            label,
            source_file: state.file_path.clone(),
            source_line: Some(line_num),
            docstring: None,
            signature: None,
            node_type: "rationale".to_string(),
        });

        state.edges.push(ExtractedEdge {
            source: rid,
            target: parent_id,
            relation: "rationale_for".to_string(),
            confidence: "EXTRACTED".to_string(),
            confidence_score: Some(1.0),
            source_file: state.file_path.clone(),
            source_line: Some(line_num),
        });
    }
}

fn find_rationale_tag(line: &str, comment_prefix: &str) -> Option<&'static str> {
    for tag in RATIONALE_TAGS {
        let pattern = format!("{} {}:", comment_prefix, tag);
        if line.starts_with(&pattern) {
            return Some(tag);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Single-file extraction
// ---------------------------------------------------------------------------

pub(crate) fn extract_single(
    path: &Path,
    cfg: &LanguageConfig,
    naming: &Path,
) -> Result<Extraction, AstriaError> {
    let source = std::fs::read(path)?;
    let source_ref = source.as_slice();

    let language = (cfg.language_fn)();
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|e| AstriaError::Parse {
            file: path.display().to_string(),
            message: e.to_string(),
        })?;

    let tree = parser
        .parse(source_ref, None)
        .ok_or_else(|| AstriaError::Parse {
            file: path.display().to_string(),
            message: "parse returned None".to_string(),
        })?;

    let root = tree.root_node();
    let fid = file_stem(naming);
    let file_id = make_node_id(&[&fid]);

    let mut state = ExtractionState {
        cfg,
        source: &source,
        file_id,
        file_path: path.to_path_buf(),
        nodes: Vec::new(),
        edges: Vec::new(),
        current_class_id: None,
        current_class_label: None,
        string_refs_seen: HashSet::new(),
        closure_counts: HashMap::new(),
        lexical_scopes: Vec::new(),
    };

    // Add file node. Rust module docs (`//!` block at the top) describe the
    // file in the language's own convention — capture them so file-level
    // questions match the file's own words.
    state.nodes.push(ExtractedNode {
        id: state.file_id.clone(),
        label: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string(),
        source_file: path.to_path_buf(),
        source_line: None,
        docstring: if cfg.name == "Rust" {
            rust_module_doc(&root, source_ref)
        } else {
            None
        },
        signature: None,
        node_type: "file".to_string(),
    });

    // Walk the AST (pass 1 structural + inline pass 2 call graph)
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        walk_structural(&mut state, &child);
    }

    // Post-pass: extract rationale comments
    extract_rationale(&mut state, &source);

    // Drop malformed edges: an unresolved reference can leave an empty
    // endpoint, and an empty target would fail build validation wholesale.
    state
        .edges
        .retain(|e| !e.source.is_empty() && !e.target.is_empty());

    // Cfg-gated twins (#[cfg(feature)] / #[cfg(not)]) textually duplicate a
    // definition; only one exists per build, so keep the first occurrence of
    // each id and drop the rest.
    let mut seen_node_ids: HashSet<String> = HashSet::new();
    state.nodes.retain(|n| seen_node_ids.insert(n.id.clone()));

    Ok(Extraction {
        file_path: path.to_path_buf(),
        language: cfg.name.to_string(),
        nodes: state.nodes,
        edges: state.edges,
    })
}
