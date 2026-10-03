// Groovy language config for the shared tree-sitter walker.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Groovy
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Groovy
            .registration()
            .extensions,
        #[cfg(feature = "lang-groovy")]
        language_fn: || tree_sitter_groovy::LANGUAGE.into(),
        #[cfg(not(feature = "lang-groovy"))]
        language_fn: || crate::langs::config::missing_language("groovy"),
        compiled_in: cfg!(feature = "lang-groovy"),
        class_types: &[
            "class_declaration",
            "interface_declaration",
            "enum_declaration",
        ],
        function_types: &[
            "method_declaration",
            "function_definition",
            "constructor_declaration",
        ],
        import_types: &["import_declaration"],
        call_type: "method_invocation",
        name_child: Some(1),
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &[],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &["closure"],
    };
    &CONFIG
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    use std::fs;

    fn extract_groovy(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("svc.groovy");
        fs::write(&file, source).unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn groovy_classes_methods_and_closures() {
        let ext = extract_groovy("import foo.Bar\n\nclass Greeter {\n  String hello(String n) {\n    return n\n  }\n}\n\ndef mk = { x -> x }\n");
        assert_eq!(ext.language, "Groovy");
        let labels: Vec<&str> = ext.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(labels.contains(&"Greeter"), "class: {labels:?}");
        assert!(labels.contains(&"hello()"), "method: {labels:?}");
        assert!(ext
            .edges
            .iter()
            .any(|e| e.relation == "imports" && e.target.contains("foo")));
        assert!(
            labels.iter().any(|l| l.contains("closure")),
            "closure: {labels:?}"
        );
    }
}
