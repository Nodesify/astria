use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Shell
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Shell
            .registration()
            .extensions,
        #[cfg(feature = "lang-shell")]
        language_fn: || tree_sitter_bash::LANGUAGE.into(),
        #[cfg(not(feature = "lang-shell"))]
        language_fn: || crate::langs::config::missing_language("shell"),
        compiled_in: cfg!(feature = "lang-shell"),
        class_types: &[], // Shell has no class system
        function_types: &["function_definition"],
        // `command` is too broad (matches every command). Shell sourcing via
        // source/. is handled at the string level in extract_import_module if needed.
        import_types: &[],
        call_type: "command",
        name_child: None,
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["compound_statement", "do_group"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
