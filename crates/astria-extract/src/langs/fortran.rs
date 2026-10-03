// Fortran language config for the shared tree-sitter walker.
// Header statements carry the `name` field; the enclosing block kinds are not registered so labels stay clean.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Fortran
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Fortran
            .registration()
            .extensions,
        #[cfg(feature = "lang-fortran")]
        language_fn: || tree_sitter_fortran::LANGUAGE.into(),
        #[cfg(not(feature = "lang-fortran"))]
        language_fn: || crate::langs::config::missing_language("fortran"),
        compiled_in: cfg!(feature = "lang-fortran"),
        class_types: &["derived_type_definition"],
        function_types: &["function_statement", "subroutine_statement"],
        import_types: &["import_statement"],
        call_type: "call_expression",
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

    fn extract_fortran(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("geometry.f90");
        fs::write(&file, source).unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn fortran_subroutines_functions_and_types() {
        let ext = extract_fortran("subroutine init()\n  implicit none\nend subroutine init\n\nfunction add(a, b) result(c)\n  add = a + b\nend function add\n\ntype :: Point\n  integer :: x\nend type Point\n");
        assert_eq!(ext.language, "Fortran");
        let labels: Vec<&str> = ext.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(labels.contains(&"init()"), "subroutine: {labels:?}");
        assert!(labels.contains(&"add()"), "function: {labels:?}");
        assert!(
            labels.iter().any(|l| l.contains("Point")),
            "derived type: {labels:?}"
        );
    }
}
