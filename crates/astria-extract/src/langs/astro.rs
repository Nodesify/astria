// Astro language config for the shared tree-sitter walker.
// Astro components are handled by langs::embedded (frontmatter TS); this config only registers the extension.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Astro
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Astro
            .registration()
            .extensions,
        #[cfg(feature = "lang-astro")]
        language_fn: || tree_sitter_astro_next::LANGUAGE.into(),
        #[cfg(not(feature = "lang-astro"))]
        language_fn: || crate::langs::config::missing_language("astro"),
        compiled_in: cfg!(feature = "lang-astro"),
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
    fn astro_config_is_registered() {
        let cfg = config();
        assert_eq!(cfg.name, "Astro");
        assert!(cfg.extensions.contains(&".astro"));
        let _ = cfg.compiled_in;
    }
}
