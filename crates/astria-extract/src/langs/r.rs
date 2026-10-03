// R language config for the shared tree-sitter walker.
// R has no class construct; library()/require() are plain calls, so imports stay empty.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::R.registration().name,
        extensions: astria_core::languages::LanguageId::R
            .registration()
            .extensions,
        #[cfg(feature = "lang-r")]
        language_fn: || tree_sitter_r::LANGUAGE.into(),
        #[cfg(not(feature = "lang-r"))]
        language_fn: || crate::langs::config::missing_language("r"),
        compiled_in: cfg!(feature = "lang-r"),
        class_types: &[],
        function_types: &["function_definition"],
        import_types: &[],
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    use std::fs;

    fn extract_r(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("stats.r");
        fs::write(&file, source).unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn r_function_definitions() {
        let ext = extract_r("greet <- function(name) {\n  print(name)\n}\n");
        assert_eq!(ext.language, "R");
        assert!(
            ext.nodes.iter().any(|n| n.label == "greet()"),
            "function: {:?}",
            ext.nodes.iter().map(|n| &n.label).collect::<Vec<_>>()
        );
    }
}
