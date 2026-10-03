pub mod astro;
pub mod c;
pub mod c_sharp;
pub mod commonlisp;
pub mod config;
pub mod css;
pub mod dart;
pub mod dm;
pub mod elixir;
pub mod embedded;
pub mod fortran;
pub mod go;
pub mod groovy;
pub mod haskell;
pub mod java;
pub mod javascript;
pub mod julia;
pub mod kotlin;
pub mod lua;
pub mod luau;
pub mod metal;
pub mod objc;
pub mod ocaml;
pub mod ocaml_interface;
pub mod pascal;
pub mod php;
pub mod powershell;
pub mod python;
pub mod r;
pub mod ruby;
pub mod rust;
pub mod scala;
pub mod shell;
pub mod solidity;
pub mod sql;
pub mod svelte;
pub mod swift;
pub mod terraform;
pub mod typescript;
pub mod vb_net;
pub mod verilog;
pub mod vue;
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
