// Julia language config for the shared tree-sitter walker.
// function/struct definitions have no `name` field; the signature/type_head is the first named child.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Julia
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Julia
            .registration()
            .extensions,
        #[cfg(feature = "lang-julia")]
        language_fn: || tree_sitter_julia::LANGUAGE.into(),
        #[cfg(not(feature = "lang-julia"))]
        language_fn: || crate::langs::config::missing_language("julia"),
        compiled_in: cfg!(feature = "lang-julia"),
        class_types: &["module_definition", "struct_definition"],
        function_types: &["function_definition", "macro_definition"],
        import_types: &["import_statement", "using_statement"],
        call_type: "call_expression",
        name_child: Some(1),
        name_field: "name",
        body_field: None,
        body_fallback_types: &["block"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &["arrow_function_expression"],
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

    fn extract_julia(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("utils.jl");
        fs::write(&file, source).unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn julia_modules_structs_and_functions() {
        let ext = extract_julia("using Printf\nmodule Utils\n\nstruct Point\n  x\nend\n\nfunction greet(name)\n  println(name)\nend\nend\n");
        assert_eq!(ext.language, "Julia");
        let joined: String = ext
            .nodes
            .iter()
            .map(|n| n.label.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(joined.contains("Utils"), "module: {joined}");
        assert!(joined.contains("Point"), "struct: {joined}");
        assert!(joined.contains("greet"), "function: {joined}");
        assert!(ext.edges.iter().any(|e| e.relation == "imports"));
    }
}
