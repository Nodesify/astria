// OCaml Interface language config for the shared tree-sitter walker.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::OcamlInterface
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::OcamlInterface
            .registration()
            .extensions,
        #[cfg(feature = "lang-ocaml-interface")]
        language_fn: || tree_sitter_ocaml::LANGUAGE_OCAML_INTERFACE.into(),
        #[cfg(not(feature = "lang-ocaml-interface"))]
        language_fn: || crate::langs::config::missing_language("ocamlinterface"),
        compiled_in: cfg!(feature = "lang-ocaml-interface"),
        class_types: &["module_definition"],
        function_types: &["value_specification"],
        import_types: &["open_module"],
        call_type: "application_expression",
        name_child: Some(1),
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    use std::fs;

    #[test]
    fn ocaml_interface_modules() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("utils.mli");
        fs::write(&file, "module Utils : sig\n  val x : int\nend\n").unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        let ext = results.remove(0);
        assert_eq!(ext.language, "OCaml Interface");
        let joined: String = ext
            .nodes
            .iter()
            .map(|n| n.label.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(joined.contains("Utils"), "module: {joined}");
    }
}
