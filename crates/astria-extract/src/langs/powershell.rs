use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "PowerShell",
        extensions: &[".ps1", ".psm1", ".psd1"],
        language_fn: || tree_sitter_powershell::LANGUAGE.into(),
        class_types: &["class_statement"],
        function_types: &["function_statement", "filter_statement"],
        import_types: &["using_statement"],
        call_type: "command_invocation",
        name_child: None,
        name_field: "command_name",
        body_field: Some("statement_block"),
        body_fallback_types: &[],
        class_call_names: &[],
        function_call_names: &["function_name"],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
