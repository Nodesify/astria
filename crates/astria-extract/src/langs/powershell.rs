use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Powershell
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Powershell
            .registration()
            .extensions,
        #[cfg(feature = "lang-powershell")]
        language_fn: || tree_sitter_powershell::LANGUAGE.into(),
        #[cfg(not(feature = "lang-powershell"))]
        language_fn: || crate::langs::config::missing_language("powershell"),
        compiled_in: cfg!(feature = "lang-powershell"),
        class_types: &["class_statement"],
        function_types: &["function_statement", "filter_statement"],
        import_types: &["using_statement"],
        call_type: "command",
        // function/class names are positional (child 1, after the keyword);
        // command nodes carry a real command_name field for call edges.
        name_child: Some(1),
        name_field: "command_name",
        body_field: Some("statement_block"),
        body_fallback_types: &[],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
