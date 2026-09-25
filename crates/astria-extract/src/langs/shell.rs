use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "Shell",
        extensions: &[".sh", ".bash"],
        language_fn: || tree_sitter_bash::LANGUAGE.into(),
        class_types: &[], // Shell has no class system
        function_types: &["function_definition"],
        // `command` is too broad (matches every command). Shell sourcing via
        // source/. is handled at the string level in extract_import_module if needed.
        import_types: &[],
        call_type: "command",
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
