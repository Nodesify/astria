// OCaml language config for the shared tree-sitter walker.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Ocaml
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Ocaml
            .registration()
            .extensions,
        #[cfg(feature = "lang-ocaml")]
        language_fn: || tree_sitter_ocaml::LANGUAGE_OCAML.into(),
        #[cfg(not(feature = "lang-ocaml"))]
        language_fn: || crate::langs::config::missing_language("ocaml"),
        compiled_in: cfg!(feature = "lang-ocaml"),
        class_types: &[
            "module_definition",
            "class_definition",
            "exception_definition",
        ],
        function_types: &["let_binding"],
        import_types: &["open_module", "include_module"],
        call_type: "application_expression",
        name_child: Some(1),
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    use std::fs;

    fn extract_ocaml(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("utils.ml");
        fs::write(&file, source).unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn ocaml_modules_and_let_bindings() {
        let ext = extract_ocaml("open Printf\n\nmodule Utils = struct\n  let x = 1\nend\n");
        assert_eq!(ext.language, "OCaml");
        let joined: String = ext
            .nodes
            .iter()
            .map(|n| n.label.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(joined.contains("Utils"), "module: {joined}");
        assert!(
            ext.nodes.iter().filter(|n| n.node_type != "file").count() >= 2,
            "module + binding: {joined}"
        );
    }
}
