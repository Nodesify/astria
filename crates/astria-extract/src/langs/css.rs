use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Css.registration().name,
        extensions: astria_core::languages::LanguageId::Css
            .registration()
            .extensions,
        #[cfg(feature = "lang-css")]
        language_fn: || tree_sitter_css::LANGUAGE.into(),
        #[cfg(not(feature = "lang-css"))]
        language_fn: || crate::langs::config::missing_language("css"),
        compiled_in: cfg!(feature = "lang-css"),
        class_types: &["rule_set"], // CSS selector blocks act as "classes"
        function_types: &[],        // CSS has no functions in the traditional sense
        import_types: &["import_statement"], // @import
        call_type: "call_expression", // CSS functions like calc(), var()
        name_child: None,
        name_field: "name", // not heavily used for CSS but consistent with API
        body_field: Some("block"),
        body_fallback_types: &[],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
