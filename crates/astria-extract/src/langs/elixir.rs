use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: "Elixir",
        extensions: &[".ex", ".exs"],
        language_fn: || tree_sitter_elixir::LANGUAGE.into(),
        // Elixir uses `call` nodes for everything: defmodule, def, defp, import, use, alias,
        // and ordinary function calls. The tree-sitter grammar doesn't distinguish them by
        // node kind, so we use class_call_names / function_call_names / import_call_names
        // to filter by the first child's text.
        class_types: &["call"],
        function_types: &["call"],
        import_types: &["call"],
        call_type: "call",
        name_field: "name",
        body_field: None,
        body_fallback_types: &["do_block", "stab_clause"],
        class_call_names: &["defmodule"],
        function_call_names: &["def", "defp", "defmacro"],
        import_call_names: &["use", "import", "alias", "require"],
        closure_types: &[],
    };
    &CONFIG
}
