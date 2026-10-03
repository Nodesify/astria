use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::CSharp
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::CSharp
            .registration()
            .extensions,
        #[cfg(feature = "lang-csharp")]
        language_fn: || tree_sitter_c_sharp::LANGUAGE.into(),
        #[cfg(not(feature = "lang-csharp"))]
        language_fn: || crate::langs::config::missing_language("csharp"),
        compiled_in: cfg!(feature = "lang-csharp"),
        class_types: &[
            "class_declaration",
            "struct_declaration",
            "interface_declaration",
            "enum_declaration",
        ],
        function_types: &[
            "method_declaration",
            "constructor_declaration",
            "local_function_statement",
        ],
        import_types: &["using_directive"],
        call_type: "invocation_expression",
        name_child: None,
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["block", "arrow_expression_clause"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
