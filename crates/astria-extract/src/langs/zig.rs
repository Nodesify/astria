use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "Zig",
        extensions: &[".zig"],
        language_fn: || tree_sitter_zig::LANGUAGE.into(),
        class_types: &[
            "struct_declaration",
            "enum_declaration",
            "union_declaration",
            "opaque_declaration",
        ],
        function_types: &["function_declaration", "test_declaration"],
        import_types: &["using_namespace_declaration"],
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
