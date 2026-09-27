use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "Terraform/HCL",
        extensions: &[".tf", ".tfvars", ".hcl"],
        language_fn: || tree_sitter_hcl::LANGUAGE.into(),
        class_types: &["block"],
        function_types: &["attribute"],
        import_types: &["get_expr"],
        call_type: "function_call",
        name_field: "identifier",
        body_field: Some("body"),
        body_fallback_types: &["object", "tuple"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
