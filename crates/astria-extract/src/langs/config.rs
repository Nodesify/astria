use tree_sitter::Language;

/// Panic path for a `language_fn` whose grammar was compiled out.
/// Never called in default builds (every `lang-*` feature is on); the
/// engine checks `compiled_in` first and skips the file instead.
pub(crate) fn missing_language(name: &str) -> ! {
    panic!("language {name} is not compiled into this build; enable its lang-* cargo feature")
}

pub struct LanguageConfig {
    pub name: &'static str,
    /// False when this build was compiled without the language's grammar
    /// (its `lang-*` cargo feature is off). The engine skips such files
    /// with a one-time warning instead of calling `language_fn` (which
    /// would hit the missing-grammar panic path).
    pub compiled_in: bool,
    pub extensions: &'static [&'static str],
    pub language_fn: fn() -> Language,
    /// Positional name fallback for grammars without named fields (HCL):
    /// the name is the nth child of the declaration node. Takes precedence
    /// over `name_field` when `name_field` is empty.
    pub name_child: Option<usize>,
    pub class_types: &'static [&'static str],
    pub function_types: &'static [&'static str],
    pub import_types: &'static [&'static str],
    pub call_type: &'static str,
    pub name_field: &'static str,
    pub body_field: Option<&'static str>,
    pub body_fallback_types: &'static [&'static str],
    /// When non-empty, only classify a node matching `class_types` as a class
    /// if the first child's text is in this list. Used for languages like Elixir
    /// where the grammar uses a single node kind for multiple constructs.
    pub class_call_names: &'static [&'static str],
    pub function_call_names: &'static [&'static str],
    pub import_call_names: &'static [&'static str],
    /// Anonymous function kinds that get synthesized names — a route-derived
    /// `VERB /path` label or a stable per-scope `{closure#N}` ordinal — and
    /// become call-attribution boundaries: calls made inside them attribute
    /// to the closure, not the enclosing function. Empty for languages that
    /// still drop anonymous functions (add kinds here to opt in).
    pub closure_types: &'static [&'static str],
}
