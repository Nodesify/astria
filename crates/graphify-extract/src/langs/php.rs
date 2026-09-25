use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "PHP",
        extensions: &[".php"],
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
    use graphify_core::db::open_db_in_memory;
    use std::fs;

    fn extract_php(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let php = dir.path().join("routes.php");
        fs::write(&php, source).unwrap();
        let db = open_db_in_memory().unwrap();
        let mut results = extract(&[php], &db).unwrap();
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
