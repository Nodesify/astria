// Vue language config for the shared tree-sitter walker.
//
// Vue SFCs are handled by langs::embedded (script blocks via the JS/TS grammars); no grammar crate is compiled in.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Vue.registration().name,
        extensions: astria_core::languages::LanguageId::Vue
            .registration()
            .extensions,
        language_fn: || crate::langs::config::missing_language("vue"),
        compiled_in: false,
        class_types: &[],
        function_types: &[],
        import_types: &[],
        call_type: "",
        name_child: None,
        name_field: "name",
        body_field: None,
        body_fallback_types: &[],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vue_config_is_registered() {
        let cfg = config();
        assert_eq!(cfg.name, "Vue");
        assert!(cfg.extensions.contains(&".vue"));
        // Extraction goes through langs::embedded, not the walker.
        assert!(!cfg.compiled_in);
    }
}
