use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "Terraform/HCL",
        extensions: &[".tf", ".tfvars", ".hcl"],
        language_fn: || tree_sitter_hcl::LANGUAGE.into(),
        class_types: &["block"],
        function_types: &[],
        import_types: &[],
        call_type: "function_call",
        name_child: Some(1),
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
