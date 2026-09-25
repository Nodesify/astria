use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "Lua",
        extensions: &[".lua"],
        language_fn: || tree_sitter_lua::LANGUAGE.into(),
        class_types: &[], // Lua has no native class system
        function_types: &["function_declaration", "function_definition"],
        // Lua uses require("module") via function_call nodes. Using function_call as
        // import_types would treat every function call as an import, so imports are
        // left empty. A future enhancement could filter by callee name == "require".
        import_types: &[],
        call_type: "function_call",
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
