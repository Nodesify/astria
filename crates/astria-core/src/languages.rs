//! Authoritative discovery names, extensions, and parser registrations.
//! The callback macro also registers extractor configs without making core depend on tree-sitter.
#[macro_export]
macro_rules! language_registry {
    ($callback:ident) => {
        $callback! {
            Python, "Python", &[".py"], python::config;
            Javascript, "JavaScript", &[".js", ".jsx", ".mjs"], javascript::config;
            Typescript, "TypeScript", &[".ts", ".tsx"], typescript::config;
            Rust, "Rust", &[".rs"], rust::config;
            Go, "Go", &[".go"], go::config;
            Java, "Java", &[".java"], java::config;
            C, "C", &[".c", ".h"], c::config;
            Cpp, "C++", &[".cpp", ".cc", ".cxx", ".hpp"], c::cpp_config;
            Ruby, "Ruby", &[".rb", ".rake"], ruby::config;
            Swift, "Swift", &[".swift"], swift::config;
            Kotlin, "Kotlin", &[".kt", ".kts"], kotlin::config;
            Scala, "Scala", &[".scala"], scala::config;
            Php, "PHP", &[".php"], php::config;
            CSharp, "C#", &[".cs"], c_sharp::config;
            Lua, "Lua", &[".lua"], lua::config;
            Haskell, "Haskell", &[".hs"], haskell::config;
            Elixir, "Elixir", &[".ex", ".exs"], elixir::config;
            Shell, "Shell", &[".sh", ".bash"], shell::config;
            Dart, "Dart", &[".dart"], dart::config;
            Zig, "Zig", &[".zig"], zig::config;
            Terraform, "Terraform/HCL", &[".tf", ".tfvars", ".hcl"], terraform::config;
            Powershell, "PowerShell", &[".ps1", ".psm1", ".psd1"], powershell::config;
            Verilog, "Verilog/SystemVerilog", &[".v", ".sv", ".svh", ".vh"], verilog::config;
            Metal, "Metal", &[".metal"], metal::config;
            Css, "CSS", &[".css", ".scss"], css::config;
        }
    };
}

pub struct LanguageRegistration {
    pub id: LanguageId,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}

macro_rules! define_languages {
    ($( $id:ident, $name:literal, $extensions:expr, $module:ident::$config:ident; )*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum LanguageId { $( $id, )* }

        impl LanguageId {
            pub const fn registration(self) -> &'static LanguageRegistration {
                match self {
                    $( Self::$id => &LanguageRegistration {
                        id: Self::$id, name: $name, extensions: $extensions,
                    }, )*
                }
            }
        }

        pub const LANGUAGES: &[&LanguageRegistration] = &[
            $( LanguageId::$id.registration(), )*
        ];
    };
}
crate::language_registry!(define_languages);

pub fn for_extension(extension: &str) -> Option<&'static LanguageRegistration> {
    let extension = extension.strip_prefix('.').unwrap_or(extension);
    LANGUAGES.iter().copied().find(|language| {
        language
            .extensions
            .iter()
            .any(|candidate| candidate[1..].eq_ignore_ascii_case(extension))
    })
}
