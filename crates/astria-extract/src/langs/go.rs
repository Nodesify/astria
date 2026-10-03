use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Go.registration().name,
        extensions: astria_core::languages::LanguageId::Go
            .registration()
            .extensions,
        #[cfg(feature = "lang-go")]
        language_fn: || tree_sitter_go::LANGUAGE.into(),
        #[cfg(not(feature = "lang-go"))]
        language_fn: || crate::langs::config::missing_language("go"),
        compiled_in: cfg!(feature = "lang-go"),
        class_types: &["type_declaration"],
        function_types: &["function_declaration", "method_declaration"],
        import_types: &["import_declaration"],
        call_type: "call_expression",
        name_child: None,
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
