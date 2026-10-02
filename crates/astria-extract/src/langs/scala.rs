use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Scala
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Scala
            .registration()
            .extensions,
        #[cfg(feature = "lang-scala")]
        language_fn: || tree_sitter_scala::LANGUAGE.into(),
        #[cfg(not(feature = "lang-scala"))]
        language_fn: || crate::langs::config::missing_language("scala"),
        compiled_in: cfg!(feature = "lang-scala"),
        class_types: &[
            "class_definition",
            "object_definition",
            "trait_definition",
            "enum_definition",
        ],
        function_types: &["function_definition", "function_declaration"],
        import_types: &["import_declaration"],
        call_type: "call_expression",
        name_child: None,
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["indented_block"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
