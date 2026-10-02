use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Haskell
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Haskell
            .registration()
            .extensions,
        #[cfg(feature = "lang-haskell")]
        language_fn: || tree_sitter_haskell::LANGUAGE.into(),
        #[cfg(not(feature = "lang-haskell"))]
        language_fn: || crate::langs::config::missing_language("haskell"),
        compiled_in: cfg!(feature = "lang-haskell"),
        class_types: &["class", "data_type", "newtype", "type_alias"],
        function_types: &["decl", "signature"],
        import_types: &["import"],
        call_type: "apply",
        name_child: None,
        name_field: "name",
        body_field: None,
        body_fallback_types: &["exp", "bind", "guard"], // Haskell bodies are expressions
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
