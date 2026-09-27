use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Terraform
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Terraform
            .registration()
            .extensions,
        language_fn: || tree_sitter_hcl::LANGUAGE.into(),
        class_types: &["block"],
        function_types: &[],
        import_types: &[],
        call_type: "function_call",
        name_child: Some(2),
        name_field: "",
        body_field: Some("body"),
        body_fallback_types: &["object", "tuple"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
