// Objective-C language config for the shared tree-sitter walker.
// Method names have no field; they are typically the second named child (after the return type).

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::ObjC.registration().name,
        extensions: astria_core::languages::LanguageId::ObjC
            .registration()
            .extensions,
        #[cfg(feature = "lang-objc")]
        language_fn: || tree_sitter_objc::LANGUAGE.into(),
        #[cfg(not(feature = "lang-objc"))]
        language_fn: || crate::langs::config::missing_language("objc"),
        compiled_in: cfg!(feature = "lang-objc"),
        class_types: &["class_implementation", "class_interface"],
        function_types: &["method_definition", "function_definition"],
        import_types: &["preproc_include", "module_import"],
        call_type: "call_expression",
        name_child: Some(2),
        name_field: "declarator",
        body_field: Some("body"),
        body_fallback_types: &["compound_statement"],
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

    fn extract_objc(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Greeter.m");
        fs::write(&file, source).unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn objc_interfaces_implementations_and_methods() {
        let ext = extract_objc("#import <Foundation/Foundation.h>\n\n@interface Greeter : NSObject\n- (void)sayHi;\n@end\n\n@implementation Greeter\n- (void)sayHi {\n  printf(\"hi\");\n}\n@end\n");
        assert_eq!(ext.language, "Objective-C");
        let joined: String = ext
            .nodes
            .iter()
            .map(|n| n.label.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(joined.contains("Greeter"), "class: {joined}");
        assert!(joined.contains("sayHi"), "method: {joined}");
    }
}
