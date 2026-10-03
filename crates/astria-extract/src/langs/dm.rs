// DreamMaker language config for the shared tree-sitter walker.
// type_definition names resolve positionally (type_path is the first named child).

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Dm.registration().name,
        extensions: astria_core::languages::LanguageId::Dm
            .registration()
            .extensions,
        #[cfg(feature = "lang-dm")]
        language_fn: || tree_sitter_dm::LANGUAGE.into(),
        #[cfg(not(feature = "lang-dm"))]
        language_fn: || crate::langs::config::missing_language("dm"),
        compiled_in: cfg!(feature = "lang-dm"),
        class_types: &["type_definition"],
        function_types: &["proc_definition", "proc_override"],
        import_types: &["preproc_include"],
        call_type: "call_expression",
        name_child: Some(1),
        name_field: "name",
        body_field: Some("block"),
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

    fn extract_dm(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chair.dm");
        fs::write(&file, source).unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn dm_types_and_procs_are_extracted() {
        let ext = extract_dm(
            "obj/chair/proc/squeak()\n\tworld << \"squeak!\"\n\n/obj/chair/verb/flip()\n\treturn\n",
        );
        assert_eq!(ext.language, "DreamMaker");
        assert!(
            ext.nodes.iter().any(|n| n.label.contains("squeak")),
            "proc: {:?}",
            ext.nodes.iter().map(|n| &n.label).collect::<Vec<_>>()
        );
    }
}
