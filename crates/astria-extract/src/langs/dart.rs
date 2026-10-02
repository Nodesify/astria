use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Dart.registration().name,
        extensions: astria_core::languages::LanguageId::Dart
            .registration()
            .extensions,
        #[cfg(feature = "lang-dart")]
        language_fn: || tree_sitter_dart::LANGUAGE.into(),
        #[cfg(not(feature = "lang-dart"))]
        language_fn: || crate::langs::config::missing_language("dart"),
        compiled_in: cfg!(feature = "lang-dart"),
        class_types: &[
            "class_definition",
            "mixin_declaration",
            "extension_declaration",
            "enum_declaration",
        ],
        function_types: &["function_expression", "method_declaration"],
        import_types: &["import_specification"],
        call_type: "call_expression",
        name_child: None,
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &["function_body"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
