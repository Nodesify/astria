use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "Rust",
        extensions: &[".rs"],
        language_fn: || tree_sitter_rust::LANGUAGE.into(),
        class_types: &["struct_item", "enum_item", "trait_item", "impl_item"],
        function_types: &["function_item", "function_signature_item"],
        import_types: &["use_declaration"],
        call_type: "call_expression",
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["block"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
