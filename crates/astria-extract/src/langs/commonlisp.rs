// Common Lisp language config for the shared tree-sitter walker.
// S-expressions have no call-shape kind we can trust (every list is a list_lit), so call edges are left off.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::CommonLisp
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::CommonLisp
            .registration()
            .extensions,
        #[cfg(feature = "lang-commonlisp")]
        language_fn: || tree_sitter_commonlisp::LANGUAGE_COMMONLISP.into(),
        #[cfg(not(feature = "lang-commonlisp"))]
        language_fn: || crate::langs::config::missing_language("commonlisp"),
        compiled_in: cfg!(feature = "lang-commonlisp"),
        class_types: &[],
        function_types: &["defun_header"],
        import_types: &[],
        call_type: "list_lit",
        name_child: None,
        name_field: "function_name",
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

    fn extract_cl(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("greeter.lisp");
        fs::write(&file, source).unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn cl_defun_becomes_a_function_node() {
        let ext = extract_cl("(defun greet (name)\n  (format t \"hi ~a~%~\" name))\n");
        assert_eq!(ext.language, "Common Lisp");
        assert!(
            ext.nodes.iter().any(|n| n.label == "greet()"),
            "defun: {:?}",
            ext.nodes.iter().map(|n| &n.label).collect::<Vec<_>>()
        );
    }
}
