use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Verilog
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Verilog
            .registration()
            .extensions,
        language_fn: || tree_sitter_systemverilog::LANGUAGE.into(),
        class_types: &[
            "module_declaration",
            "interface_declaration",
            "program_declaration",
            "class_declaration",
        ],
        function_types: &["function_body_declaration", "task_body_declaration"],
        import_types: &["import_declaration"],
        call_type: "subroutine_call",
        name_child: None,
        name_field: "name",
        body_field: Some("module_item"),
        body_fallback_types: &[],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
