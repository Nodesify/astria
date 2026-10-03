use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Ruby.registration().name,
        extensions: astria_core::languages::LanguageId::Ruby
            .registration()
            .extensions,
        #[cfg(feature = "lang-ruby")]
        language_fn: || tree_sitter_ruby::LANGUAGE.into(),
        #[cfg(not(feature = "lang-ruby"))]
        language_fn: || crate::langs::config::missing_language("ruby"),
        compiled_in: cfg!(feature = "lang-ruby"),
        class_types: &["class", "module", "singleton_class"],
        function_types: &["method", "singleton_method"],
        import_types: &["call"],
        call_type: "call",
        name_child: None,
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["body_statement", "do"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
