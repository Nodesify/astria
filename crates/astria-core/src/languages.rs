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
            Astro, "Astro", &[".astro"], astro::config;
            CommonLisp, "Common Lisp", &[".lisp", ".cl", ".lsp", ".asd"], commonlisp::config;
            Dm, "DreamMaker", &[".dm"], dm::config;
            Fortran, "Fortran", &[".f90", ".f95", ".f03", ".f08", ".f", ".for"], fortran::config;
            Groovy, "Groovy", &[".groovy", ".gradle"], groovy::config;
            Julia, "Julia", &[".jl"], julia::config;
            Luau, "Luau", &[".luau"], luau::config;
            ObjC, "Objective-C", &[".m", ".mm"], objc::config;
            Ocaml, "OCaml", &[".ml"], ocaml::config;
            OcamlInterface, "OCaml Interface", &[".mli"], ocaml_interface::config;
            R, "R", &[".r"], r::config;
            Solidity, "Solidity", &[".sol"], solidity::config;
            Sql, "SQL", &[".sql"], sql::config;
            VbNet, "VB.NET", &[".vb"], vb_net::config;
            Pascal, "Pascal/Delphi", &[".pas", ".dpr", ".dpk", ".inc"], pascal::config;
            Vue, "Vue", &[".vue"], vue::config;
            Svelte, "Svelte", &[".svelte"], svelte::config;
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

/// Project-level extension overrides, e.g. ".inc" -> PHP for a repo whose
/// includes are PHP sources. Set from the project's `.astria.toml` at the
/// start of every pipeline run and graph-loading command, so discovery,
/// parser selection, watch, and cache identity all agree on one mapping.
/// Stored per-process (not passed call-by-call) because read-only commands
/// must inherit the same map the build used.
type OverrideMap = std::collections::HashMap<String, LanguageId>;
static EXTENSION_OVERRIDES: std::sync::RwLock<Option<OverrideMap>> = std::sync::RwLock::new(None);

pub fn set_extension_overrides(overrides: OverrideMap) {
    if let Ok(mut guard) = EXTENSION_OVERRIDES.write() {
        *guard = Some(overrides);
    }
}

pub fn clear_extension_overrides() {
    if let Ok(mut guard) = EXTENSION_OVERRIDES.write() {
        *guard = None;
    }
}

/// Validate a project's extension→language mapping (the `[languages]`
/// section of `.astria.toml`). Returns the valid overrides plus a warning
/// for each rejected entry — a partial config applies its valid part
/// instead of failing the whole build over one typo. Extensions normalize
/// to lowercase with the leading dot stripped (matching `for_extension`'s
/// lookup); language names must match a registry language
/// case-insensitively.
pub fn parse_extension_overrides(entries: &[(String, String)]) -> (OverrideMap, Vec<String>) {
    let mut map = OverrideMap::new();
    let mut warnings = Vec::new();
    for (raw_extension, raw_language) in entries {
        let mut extension = raw_extension.trim().to_ascii_lowercase();
        if !extension.starts_with('.') {
            extension.insert(0, '.');
        }
        let stripped = extension[1..].to_string();
        let ext_ok = !stripped.is_empty()
            && stripped.len() <= 16
            && stripped
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
        match LANGUAGES
            .iter()
            .copied()
            .find(|l| l.name.eq_ignore_ascii_case(raw_language.trim()))
        {
            Some(language) if ext_ok => {
                map.insert(stripped, language.id);
            }
            Some(_) => warnings.push(format!(
                "languages: '.{stripped}' is not a usable extension (1-16 alphanumeric/-/_ characters)"
            )),
            None => warnings.push(format!(
                "languages: '{raw_language}' is not a known language — see the registry names (e.g. PHP, TypeScript, Pascal/Delphi)"
            )),
        }
    }
    (map, warnings)
}

/// The project override for an extension, if one is set. Hardcoded
/// extraction fallbacks (embedded component files, regex extractors for
/// languages without grammar crates) yield to overrides so a remapped
/// suffix reaches its mapped parser.
pub fn extension_override(extension: &str) -> Option<&'static LanguageRegistration> {
    let extension = extension.strip_prefix('.').unwrap_or(extension);
    let lower = extension.to_ascii_lowercase();
    let guard = EXTENSION_OVERRIDES.read().ok()?;
    let map = guard.as_ref()?;
    let id = map.get(lower.as_str()).copied()?;
    Some(id.registration())
}

pub fn for_extension(extension: &str) -> Option<&'static LanguageRegistration> {
    let extension = extension.strip_prefix('.').unwrap_or(extension);
    let lower = extension.to_ascii_lowercase();
    if let Ok(guard) = EXTENSION_OVERRIDES.read() {
        if let Some(map) = guard.as_ref() {
            if let Some(&id) = map.get(lower.as_str()) {
                return Some(id.registration());
            }
        }
    }
    LANGUAGES.iter().copied().find(|language| {
        language
            .extensions
            .iter()
            .any(|candidate| candidate[1..].eq_ignore_ascii_case(&lower))
    })
}

/// Resolve extensionless scripts from a bounded, complete shebang line.
/// Extension mappings take precedence; arbitrary content is never guessed.
pub fn for_path(path: &std::path::Path) -> Option<&'static LanguageRegistration> {
    if let Some(extension) = path.extension() {
        return for_extension(extension.to_str()?);
    }
    use std::io::Read;
    let mut prefix = [0u8; 512];
    let count = std::fs::File::open(path).ok()?.read(&mut prefix).ok()?;
    let end = prefix[..count]
        .iter()
        .position(|&b| b == b'\n')
        .or_else(|| (count < prefix.len()).then_some(count))?;
    for_shebang(std::str::from_utf8(&prefix[..end]).ok()?)
}

pub fn for_shebang(line: &str) -> Option<&'static LanguageRegistration> {
    let mut words = line.strip_prefix("#!")?.split_ascii_whitespace();
    let executable = words.next()?;
    let mut interpreter = executable.rsplit('/').next()?;
    if interpreter == "env" {
        interpreter = words.next()?;
        if interpreter == "-S" || interpreter == "--split-string" {
            interpreter = words.next()?;
        }
        // Only explicit env assignments are skipped; unknown options fail closed.
        while interpreter.contains('=') && !interpreter.starts_with('-') {
            let key = interpreter.split_once('=')?.0;
            if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return None;
            }
            interpreter = words.next()?;
        }
    }
    let interpreter = interpreter.rsplit('/').next()?;
    let interpreters = [
        ("python", LanguageId::Python),
        ("node", LanguageId::Javascript),
        ("nodejs", LanguageId::Javascript),
        ("ruby", LanguageId::Ruby),
        ("sh", LanguageId::Shell),
        ("bash", LanguageId::Shell),
        ("dash", LanguageId::Shell),
        ("ksh", LanguageId::Shell),
        ("php", LanguageId::Php),
        ("lua", LanguageId::Lua),
        ("swift", LanguageId::Swift),
        ("julia", LanguageId::Julia),
        ("Rscript", LanguageId::R),
        ("pwsh", LanguageId::Powershell),
        ("powershell", LanguageId::Powershell),
    ];
    interpreters.into_iter().find_map(|(name, id)| {
        let version = interpreter.strip_prefix(name)?;
        // Permit an exact executable or a numeric dotted version, never an
        // arbitrary shared prefix such as python-config or bash-helper.
        let matches = version.is_empty()
            || version
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
        matches.then_some(id.registration())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The override state is process-global; tests that touch it serialize
    /// on this lock so they cannot race each other or the registry tests.
    static OVERRIDE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn parse_extension_overrides_accepts_and_normalizes() {
        let (map, warnings) = parse_extension_overrides(&[
            (".inc".to_string(), "PHP".to_string()),
            ("tpl".to_string(), "typescript".to_string()),
        ]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(map.get("inc"), Some(&LanguageId::Php));
        assert_eq!(map.get("tpl"), Some(&LanguageId::Typescript));

        let (_, warnings) = parse_extension_overrides(&[
            (".x".to_string(), "Not A Language".to_string()),
            (".has space".to_string(), "PHP".to_string()),
            ("".to_string(), "PHP".to_string()),
        ]);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
    }

    #[test]
    fn overrides_steer_extension_lookup_until_cleared() {
        let _guard = OVERRIDE_LOCK.lock().unwrap();
        // Built-in: .inc is Pascal/Delphi by default; .tpl is unknown.
        assert_eq!(for_extension(".inc").map(|l| l.name), Some("Pascal/Delphi"));
        assert_eq!(for_extension(".tpl").map(|l| l.name), None);

        let (map, warnings) = parse_extension_overrides(&[
            (".inc".to_string(), "PHP".to_string()),
            (".tpl".to_string(), "TypeScript".to_string()),
        ]);
        assert!(warnings.is_empty());
        set_extension_overrides(map);
        assert_eq!(for_extension(".inc").map(|l| l.name), Some("PHP"));
        assert_eq!(for_extension(".tpl").map(|l| l.name), Some("TypeScript"));
        // Registered extensions still resolve when not overridden.
        assert_eq!(for_extension(".rs").map(|l| l.name), Some("Rust"));

        clear_extension_overrides();
        assert_eq!(for_extension(".inc").map(|l| l.name), Some("Pascal/Delphi"));
        assert_eq!(for_extension(".tpl").map(|l| l.name), None);
    }
}
