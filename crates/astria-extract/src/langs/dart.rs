use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "Dart",
        extensions: &[".dart"],
        language_fn: || tree_sitter_dart::LANGUAGE.into(),
        class_types: &[
            "class_definition",
            "mixin_declaration",
            "extension_declaration",
            "enum_declaration",
        ],
        function_types: &["function_expression", "method_declaration"],
        import_types: &["import_specification"],
        call_type: "call_expression",
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["function_body"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
