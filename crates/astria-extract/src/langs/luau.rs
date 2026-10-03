// Luau language config for the shared tree-sitter walker.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Luau.registration().name,
        extensions: astria_core::languages::LanguageId::Luau
            .registration()
            .extensions,
        #[cfg(feature = "lang-luau")]
        language_fn: || tree_sitter_luau::LANGUAGE.into(),
        #[cfg(not(feature = "lang-luau"))]
        language_fn: || crate::langs::config::missing_language("luau"),
        compiled_in: cfg!(feature = "lang-luau"),
        class_types: &[],
        function_types: &["function_declaration", "function_definition"],
        import_types: &[],
        call_type: "function_call",
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

    fn extract_luau(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("init.luau");
        fs::write(&file, source).unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn luau_functions_are_extracted() {
        let ext =
            extract_luau("local function helper()\n\tend\n\nfunction Handler.onEvent()\n\tend\n");
        assert_eq!(ext.language, "Luau");
        let joined: String = ext
            .nodes
            .iter()
            .map(|n| n.label.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(joined.contains("onEvent"), "declaration: {joined}");
        assert!(joined.contains("helper"), "local definition: {joined}");
    }
}
