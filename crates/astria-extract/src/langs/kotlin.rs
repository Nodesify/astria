use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Kotlin
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Kotlin
            .registration()
            .extensions,
        #[cfg(feature = "lang-kotlin")]
        language_fn: || tree_sitter_kotlin_ng::LANGUAGE.into(),
        #[cfg(not(feature = "lang-kotlin"))]
        language_fn: || crate::langs::config::missing_language("kotlin"),
        compiled_in: cfg!(feature = "lang-kotlin"),
        class_types: &[
            "class_declaration",
            "object_declaration",
            "interface_declaration",
            "enum_declaration",
        ],
        function_types: &["function_declaration"],
        import_types: &["import_header"],
        call_type: "call_expression",
        name_child: None,
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
