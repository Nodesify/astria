use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "Python",
        extensions: &[".py"],
        language_fn: || tree_sitter_python::LANGUAGE.into(),
        class_types: &["class_definition"],
        function_types: &["function_definition"],
        import_types: &["import_statement", "import_from_statement"],
        call_type: "call",
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
