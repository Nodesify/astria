use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "Haskell",
        extensions: &[".hs"],
        language_fn: || tree_sitter_haskell::LANGUAGE.into(),
        class_types: &["class", "data_type", "newtype", "type_alias"],
        function_types: &["decl", "signature"],
        import_types: &["import"],
        call_type: "apply",
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
