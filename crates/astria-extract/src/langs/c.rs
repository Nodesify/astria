use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::C.registration().name,
        extensions: astria_core::languages::LanguageId::C
            .registration()
            .extensions,
        #[cfg(feature = "lang-c")]
        language_fn: || tree_sitter_c::LANGUAGE.into(),
        #[cfg(not(feature = "lang-c"))]
        language_fn: || crate::langs::config::missing_language("c"),
        compiled_in: cfg!(feature = "lang-c"),
        class_types: &["struct_specifier", "enum_specifier"],
        function_types: &["function_definition"],
        import_types: &["preproc_include"],
        call_type: "call_expression",
        name_child: None,
        name_field: "declarator",
        body_field: Some("body"),
        body_fallback_types: &["compound_statement"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}

pub fn cpp_config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Cpp.registration().name,
        name_child: None,
        extensions: astria_core::languages::LanguageId::Cpp
            .registration()
            .extensions,
        #[cfg(feature = "lang-cpp")]
        language_fn: || tree_sitter_cpp::LANGUAGE.into(),
        #[cfg(not(feature = "lang-cpp"))]
        language_fn: || crate::langs::config::missing_language("cpp"),
        compiled_in: cfg!(feature = "lang-cpp"),
        class_types: &["class_specifier", "struct_specifier", "enum_specifier"],
        function_types: &["function_definition"],
        import_types: &["preproc_include"],
        call_type: "call_expression",
        name_field: "declarator",
        body_field: Some("body"),
        body_fallback_types: &["compound_statement"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}
