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
pub mod metal;
pub mod php;
pub mod powershell;
pub mod python;
pub mod ruby;
pub mod rust;
pub mod scala;
pub mod shell;
pub mod swift;
pub mod terraform;
pub mod typescript;
pub mod verilog;
pub mod zig;

pub use config::LanguageConfig;

macro_rules! register_configs {
    ($( $id:ident, $name:literal, $extensions:expr, $module:ident::$config:ident; )*) => {
        pub fn get_language_for_extension(ext: &str) -> Option<&'static LanguageConfig> {
            use astria_core::languages::{for_extension, LanguageId};
            Some(match for_extension(ext)?.id {
                $( LanguageId::$id => $module::$config(), )*
            })
        }

        pub fn all_languages() -> Vec<&'static LanguageConfig> {
            vec![$( $module::$config(), )*]
        }
    };
}
astria_core::language_registry!(register_configs);
