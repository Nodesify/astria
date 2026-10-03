// Svelte language config for the shared tree-sitter walker.
//
// Svelte components are handled by langs::embedded (script blocks via the JS/TS grammars); no grammar crate is compiled in.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Svelte
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Svelte
            .registration()
            .extensions,
        language_fn: || crate::langs::config::missing_language("svelte"),
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
    fn svelte_config_is_registered() {
        let cfg = config();
        assert_eq!(cfg.name, "Svelte");
        assert!(cfg.extensions.contains(&".svelte"));
        // Extraction goes through langs::embedded, not the walker.
        assert!(!cfg.compiled_in);
    }
}
