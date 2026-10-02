use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Terraform
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Terraform
            .registration()
            .extensions,
        #[cfg(feature = "lang-terraform")]
        language_fn: || tree_sitter_hcl::LANGUAGE.into(),
        #[cfg(not(feature = "lang-terraform"))]
        language_fn: || crate::langs::config::missing_language("terraform"),
        compiled_in: cfg!(feature = "lang-terraform"),
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
