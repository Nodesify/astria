use super::config::LanguageConfig;

/// Metal shaders are C++-based; the C++ tree-sitter grammar extracts
/// vertex/fragment/kernel functions well enough for symbol coverage.
pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Metal
            .registration()
            .name,
        name_child: None,
        extensions: astria_core::languages::LanguageId::Metal
            .registration()
            .extensions,
        language_fn: || tree_sitter_cpp::LANGUAGE.into(),
        class_types: &["struct_specifier", "class_specifier"],
        function_types: &["function_definition"],
        import_types: &["preproc_include"],
        call_type: "call_expression",
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
