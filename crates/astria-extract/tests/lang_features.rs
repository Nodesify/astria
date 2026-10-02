//! Feature-gate sanity: with the default feature set (`lang-all`), every
//! registered language config must report its grammar as compiled in. A
//! failure here means a config was registered but its `lang-*` feature or
//! grammar dependency drifted — the engine would silently skip those files.

#[test]
fn every_registered_language_is_compiled_in_by_default() {
    for cfg in astria_extract::langs::all_languages() {
        assert!(
            cfg.compiled_in,
            "language {} is registered but its grammar is not compiled in",
            cfg.name
        );
    }
}

#[test]
fn language_count_matches_the_documented_registry() {
    // 25 configurations per README/docs (C and C++ share one file; Metal
    // shares the C++ grammar). Keep this in sync with the language-support
    // docs — the docs-sync CI guard regenerates the table from the registry.
    let count = astria_extract::langs::all_languages().len();
    assert_eq!(count, 25, "registered language configs");
}
