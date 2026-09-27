use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Java.registration().name,
        extensions: astria_core::languages::LanguageId::Java
            .registration()
            .extensions,
        language_fn: || tree_sitter_java::LANGUAGE.into(),
        class_types: &[
            "class_declaration",
            "interface_declaration",
            "enum_declaration",
        ],
        function_types: &["method_declaration", "constructor_declaration"],
        import_types: &["import_declaration"],
        call_type: "method_invocation",
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
