//! Feature-gate sanity: with the default feature set (`lang-all`), every
//! registered language config must report its grammar as compiled in. A
//! failure here means a config was registered but its `lang-*` feature or
//! grammar dependency drifted — the engine would silently skip those files.
//!
//! Exception: routed languages. Vue, Svelte, VB.NET and Pascal/Delphi are
//! registered (so detect/classify and the docs generator see them) but their
//! extraction never goes through the tree-sitter walker — the engine routes
//! them to the embedded-script / regex extractors instead, and they ship no
//! grammar crate.

/// Languages that are intentionally grammarless: the engine routes them to a
/// dedicated extractor before the walker would ever see them.
const ROUTED_WITHOUT_GRAMMAR: &[&str] = &["Vue", "Svelte", "VB.NET", "Pascal/Delphi"];

#[test]
fn every_registered_language_is_compiled_in_by_default() {
    for cfg in astria_extract::langs::all_languages() {
        if ROUTED_WITHOUT_GRAMMAR.contains(&cfg.name) {
            assert!(
                !cfg.compiled_in,
                "language {} is documented as routed-without-grammar but reports a compiled grammar",
                cfg.name
            );
            continue;
        }
        assert!(
            cfg.compiled_in,
            "language {} is registered but its grammar is not compiled in",
            cfg.name
        );
    }
}

#[test]
fn language_count_matches_the_documented_registry() {
    // 25 original configurations + 17 added in the ingestion expansion
    // (Julia, R, Fortran, Solidity, Groovy, Luau, OCaml, OCaml Interface,
    // Objective-C, Common Lisp, DreamMaker, SQL, Astro, VB.NET, Pascal,
    // Vue, Svelte). C and C++ share one file; Metal shares the C++ grammar.
    // Keep this in sync with the language-support docs — the docs-sync CI
    // guard regenerates the table from the registry.
    let count = astria_extract::langs::all_languages().len();
    assert_eq!(count, 42, "registered language configs");
}
