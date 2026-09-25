pub mod c;
pub mod c_sharp;
pub mod config;
pub mod css;
pub mod dart;
pub mod elixir;
pub mod go;
pub mod haskell;
pub mod java;
pub mod javascript;
pub mod kotlin;
pub mod lua;
pub mod php;
pub mod python;
pub mod ruby;
pub mod rust;
pub mod scala;
pub mod shell;
pub mod swift;
pub mod typescript;
pub mod zig;

pub use config::LanguageConfig;

pub fn get_language_for_extension(ext: &str) -> Option<&'static LanguageConfig> {
    let ext_with_dot = if ext.starts_with('.') {
        ext.to_lowercase()
    } else {
        format!(".{}", ext).to_lowercase()
    };
    all_languages()
        .iter()
        .find(|cfg| cfg.extensions.contains(&ext_with_dot.as_str()))
        .copied()
}

pub fn all_languages() -> Vec<&'static LanguageConfig> {
    vec![
        python::config(),
        javascript::config(),
        typescript::config(),
        rust::config(),
        go::config(),
        java::config(),
        c::config(),
        c::cpp_config(),
        ruby::config(),
        swift::config(),
        kotlin::config(),
        scala::config(),
        php::config(),
        c_sharp::config(),
        lua::config(),
        haskell::config(),
        elixir::config(),
        shell::config(),
        dart::config(),
        zig::config(),
        css::config(),
    ]
}
