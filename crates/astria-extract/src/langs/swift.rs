use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Swift
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Swift
            .registration()
            .extensions,
        #[cfg(feature = "lang-swift")]
        language_fn: || tree_sitter_swift::LANGUAGE.into(),
        #[cfg(not(feature = "lang-swift"))]
        language_fn: || crate::langs::config::missing_language("swift"),
        compiled_in: cfg!(feature = "lang-swift"),
        class_types: &[
            "class_declaration",
            "struct_declaration",
            "enum_declaration",
            "protocol_declaration",
        ],
        function_types: &["function_declaration"],
        import_types: &["import_declaration"],
        call_type: "call_expression",
        name_child: None,
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["class_body", "enum_class_body"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
