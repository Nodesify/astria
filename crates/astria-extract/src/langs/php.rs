use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Php.registration().name,
        extensions: astria_core::languages::LanguageId::Php
            .registration()
            .extensions,
        language_fn: || tree_sitter_php::LANGUAGE_PHP.into(),
        class_types: &[
            "class_declaration",
            "interface_declaration",
            "trait_declaration",
            "enum_declaration",
        ],
        function_types: &[
            "function_definition",
            "method_declaration",
            "declaration_list",
        ],
        import_types: &["namespace_use_declaration", "namespace_definition"],
        call_type: "function_call_expression",
        name_child: None,
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["compound_statement", "declaration_list"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &["anonymous_function", "arrow_function"],
    };
    &CONFIG
}

#[cfg(test)]
mod tests {
    use crate::engine::extract;
    use crate::naming::make_target_id;
    use astria_core::db::open_db_in_memory;
    use std::fs;

    fn extract_php(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let php = dir.path().join("routes.php");
        fs::write(&php, source).unwrap();
        let db = open_db_in_memory().unwrap();
        let mut results = extract(&[php], dir.path(), &db).unwrap();
        // Keep the tempdir alive for the returned Extraction's paths.
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    fn closure_labels(ext: &crate::schema::Extraction) -> Vec<String> {
        ext.nodes
            .iter()
            .filter(|n| n.label.contains("{closure#"))
            .map(|n| n.label.clone())
            .collect()
    }

    /// Call edges may target the raw bare name or the cross-file-resolved
    /// definition id (refs.rs rewrites targets when exactly one definition
    /// matches). Accept either for a named callee.
    fn caller_ids_of(ext: &crate::schema::Extraction, callee_label: &str) -> Vec<String> {
        let callee_bare = callee_label.trim_end_matches("()");
        let mut targets: Vec<String> = vec![make_target_id(callee_bare)];
        targets.extend(
            ext.nodes
                .iter()
                .filter(|n| n.label == callee_label)
                .map(|n| n.id.clone()),
        );
        ext.edges
            .iter()
            .filter(|e| e.relation == "calls" && targets.contains(&e.target))
            .map(|e| e.source.clone())
            .collect()
    }

    #[test]
    fn route_closure_gets_semantic_name() {
        let ext = extract_php(
            "<?php\n$app->get('/api/users', function() {\n    return [];\n});\n$app->post('/api/users', function() {\n    return 'created';\n});\n",
        );
        let labels: Vec<&str> = ext.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.contains(&"GET /api/users()"),
            "expected route closure label 'GET /api/users()', got {labels:?}"
        );
        assert!(
            labels.contains(&"POST /api/users()"),
            "expected route closure label 'POST /api/users()', got {labels:?}"
        );
    }

    #[test]
    fn generic_closure_gets_ordinal_name() {
        let ext = extract_php(
            "<?php\n$fn1 = fn($x) => $x + 1;\n$fn2 = function() { return 'hello'; };\n",
        );
        let labels = closure_labels(&ext);
        // Ordinal labels carry their scope qualifier (file stem at file
        // scope), which is tempdir-dependent here — match on the ordinal part.
        assert!(
            labels.iter().any(|l| l.ends_with("{closure#1}()")),
            "expected first generic closure to end with '{{closure#1}}()', got {labels:?}"
        );
        assert!(
            labels.iter().any(|l| l.ends_with("{closure#2}()")),
            "expected second generic closure to end with '{{closure#2}}()', got {labels:?}"
        );
        assert!(
            !ext.nodes.iter().any(|n| n.label.contains("closure@")),
            "line-based closure names should not appear"
        );
    }

    #[test]
    fn nested_route_closure_composes_prefix() {
        let ext = extract_php(
            "<?php\n$app->group('/api/v1', function ($group) {\n    $group->get('/users/{id}', function ($req, $res) { return 1; });\n});\n",
        );
        let labels: Vec<&str> = ext.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.contains(&"GET /api/v1/users/{id}()"),
            "expected nested route closure to compose prefix, got {labels:?}"
        );
        assert!(
            labels.iter().any(|l| l.ends_with("{closure#1}()")),
            "expected outer group closure to fall back to ordinal, got {labels:?}"
        );
    }

    #[test]
    fn cache_get_avoids_route_false_positive() {
        let ext =
            extract_php("<?php\n$value = $cache->get('user:42', function () { return 2; });\n");
        let labels = closure_labels(&ext);
        assert!(
            labels.iter().any(|l| l.ends_with("{closure#1}()")),
            "expected non-routing get() to fall back to ordinal, got {labels:?}"
        );
        assert!(
            !ext.nodes.iter().any(|n| n.label.starts_with("GET ")),
            "expected no route label for cache method"
        );
    }

    #[test]
    fn file_scope_arg_closure_call_attributes_to_closure() {
        let ext = extract_php(
            "<?php\nfunction handler($x) { return $x; }\n$assigned = function($x) { return handler($x); };\narray_map(function($y){ return handler($y); }, $items);\n",
        );
        let closure_ids: Vec<String> = ext
            .nodes
            .iter()
            .filter(|n| n.label.contains("{closure#"))
            .map(|n| n.id.clone())
            .collect();
        assert!(
            closure_ids.len() >= 2,
            "expected file-scope closures as nodes, got {:?}",
            ext.nodes.iter().map(|n| &n.label).collect::<Vec<_>>()
        );
        let handler_callers = caller_ids_of(&ext, "handler()");
        assert!(
            !handler_callers.is_empty(),
            "closure inner calls to handler() were not captured"
        );
        assert!(
            handler_callers.iter().all(|src| closure_ids.contains(src)),
            "handler() calls must attribute to closures, not the file: {handler_callers:?}"
        );
    }

    #[test]
    fn closures_produce_no_duplicate_nodes() {
        let ext = extract_php(
            "<?php\n$a = function() { return 1; };\n$b = fn() => 2;\narray_map(function() { return 3; }, []);\n",
        );
        let mut counts = std::collections::HashMap::new();
        for label in closure_labels(&ext) {
            *counts.entry(label).or_insert(0usize) += 1;
        }
        assert!(!counts.is_empty(), "expected closure nodes to be extracted");
        assert!(
            counts.values().all(|&c| c == 1),
            "duplicate closure nodes: {counts:?}"
        );
    }

    #[test]
    fn method_scope_closure_call_attributes_to_closure_not_method() {
        let ext = extract_php(
            "<?php\nfunction target($x) { return $x; }\nclass Service {\n    public function run() {\n        $cb = function($x) { return target($x); };\n        return $cb;\n    }\n}\n",
        );
        let closure_ids: Vec<String> = ext
            .nodes
            .iter()
            .filter(|n| n.label.contains("{closure#"))
            .map(|n| n.id.clone())
            .collect();
        let target_callers = caller_ids_of(&ext, "target()");
        assert!(
            !target_callers.is_empty(),
            "call to target() inside the method-level closure was not captured"
        );
        assert!(
            target_callers.iter().all(|src| closure_ids.contains(src)),
            "target() must attribute to the closure, not run(): {target_callers:?}"
        );
        let run_node = ext
            .nodes
            .iter()
            .find(|n| n.label == "run()")
            .expect("run() method node should exist");
        assert!(
            !target_callers.contains(&run_node.id),
            "target() must not attribute to the enclosing run() method"
        );
    }
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

/// PHP routing verbs whose first string argument is a route path. A closure
/// passed directly to one of these gets a `VERB /path` label instead of an
/// ordinal (#3409).
pub(crate) const PHP_ROUTING_VERBS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "options", "any", "match", "map",
];

/// First string literal among `arg_list`'s children, stopping at `before`
/// (the argument the closure itself sits in) when given. Handles both bare
/// string children and `argument`-wrapped strings.
pub(crate) fn first_string_arg<'a>(
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
pub(crate) fn php_route_name(closure: &Node<'_>, source: &[u8]) -> Option<String> {
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
