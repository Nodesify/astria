use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "Ruby",
        extensions: &[".rb", ".rake"],
        language_fn: || tree_sitter_ruby::LANGUAGE.into(),
        class_types: &["class", "module", "singleton_class"],
        function_types: &["method", "singleton_method"],
        import_types: &["call"],
        call_type: "call",
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["body_statement", "do"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
