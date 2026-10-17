// build.rs: derive the grammar-version table for the extraction cache
// fingerprint from Cargo.lock. A grammar upgrade must invalidate only the
// cached extractions of the language that grammar parses — not the whole
// corpus and not unrelated languages — so the per-language versions (and
// the tree-sitter core version, which affects every grammar) are baked
// into a generated table the cache keys mix in.
//
// The lockfile is the source of truth because it is what actually built:
// a `cargo update` bump shows up here without any manual version edit.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Extraction language name (as the language registry spells it) → the
/// grammar crate that parses it. Keep in sync with the configs in
/// src/langs/: a language parsed by another language's grammar (Metal via
/// C++) maps to that crate. Languages that never reach the AST extraction
/// cache (docs, manifests, embedded components, regex fallbacks) are not
/// listed.
const LANGUAGE_GRAMMARS: &[(&str, &str)] = &[
    ("Python", "tree-sitter-python"),
    ("JavaScript", "tree-sitter-javascript"),
    ("TypeScript", "tree-sitter-typescript"),
    ("Rust", "tree-sitter-rust"),
    ("Go", "tree-sitter-go"),
    ("Java", "tree-sitter-java"),
    ("C", "tree-sitter-c"),
    ("C++", "tree-sitter-cpp"),
    ("Metal", "tree-sitter-cpp"),
    ("Ruby", "tree-sitter-ruby"),
    ("Swift", "tree-sitter-swift"),
    ("Kotlin", "tree-sitter-kotlin-ng"),
    ("Scala", "tree-sitter-scala"),
    ("PHP", "tree-sitter-php"),
    ("C#", "tree-sitter-c-sharp"),
    ("Lua", "tree-sitter-lua"),
    ("Haskell", "tree-sitter-haskell"),
    ("Elixir", "tree-sitter-elixir"),
    ("Shell", "tree-sitter-bash"),
    ("Dart", "tree-sitter-dart"),
    ("Zig", "tree-sitter-zig"),
    ("Terraform/HCL", "tree-sitter-hcl"),
    ("PowerShell", "tree-sitter-powershell"),
    ("Verilog/SystemVerilog", "tree-sitter-systemverilog"),
    ("CSS", "tree-sitter-css"),
    ("Common Lisp", "tree-sitter-commonlisp"),
    ("DreamMaker", "tree-sitter-dm"),
    ("Fortran", "tree-sitter-fortran"),
    ("Groovy", "tree-sitter-groovy"),
    ("Julia", "tree-sitter-julia"),
    ("Luau", "tree-sitter-luau"),
    ("Objective-C", "tree-sitter-objc"),
    ("OCaml", "tree-sitter-ocaml"),
    ("OCaml Interface", "tree-sitter-ocaml"),
    ("R", "tree-sitter-r"),
    ("Solidity", "tree-sitter-solidity"),
    ("SQL", "tree-sitter-sequel"),
];

/// Walk up from the manifest dir to the workspace Cargo.lock. Standalone
/// builds (no lockfile) fall back to "unknown" versions: the fingerprint
/// stays stable, just no longer version-sensitive.
fn find_lockfile(manifest_dir: &Path) -> Option<PathBuf> {
    let mut dir = Some(manifest_dir.to_path_buf());
    while let Some(current) = dir {
        let candidate = current.join("Cargo.lock");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = current.parent().map(Path::to_path_buf);
    }
    None
}

/// Package versions from the lockfile, filtered to tree-sitter crates:
/// `{"tree-sitter-rust": "0.23.2", ...}`.
fn tree_sitter_versions(lock_path: &Path) -> std::collections::HashMap<String, String> {
    let mut versions = std::collections::HashMap::new();
    let Ok(text) = fs::read_to_string(lock_path) else {
        return versions;
    };
    let mut pending_name: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("[[package]]") {
            pending_name = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("name = ") {
            pending_name = Some(rest.trim_matches('"').to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("version = ") {
            if let Some(name) = &pending_name {
                if name == "tree-sitter" || name.starts_with("tree-sitter-") {
                    versions.insert(name.clone(), rest.trim_matches('"').to_string());
                }
            }
        }
    }
    versions
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let lockfile = find_lockfile(&manifest_dir);
    if let Some(lock) = &lockfile {
        // A grammar bump reaches the fingerprint through the lockfile:
        // without this, a `cargo update` would rebuild with new grammars
        // while the generated table (and cache keys) still showed the old
        // versions.
        println!("cargo:rerun-if-changed={}", lock.display());
    }
    println!("cargo:rerun-if-changed=build.rs");

    let versions = tree_sitter_versions(lockfile.as_deref().unwrap_or(Path::new("")));
    let core = versions
        .get("tree-sitter")
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());

    let mut rows: Vec<(String, String)> = LANGUAGE_GRAMMARS
        .iter()
        .map(|&(language, crate_name)| {
            let version = versions
                .get(crate_name)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            (language.to_string(), format!("{crate_name}@{version}"))
        })
        .collect();
    rows.sort();

    let mut table = String::from(
        "// Generated by build.rs from Cargo.lock — do not edit.\n\
         pub(crate) const TREE_SITTER_CORE: &str = \"tree-sitter@",
    );
    table.push_str(&core);
    table.push_str("\";\n");
    table.push_str("pub(crate) const GRAMMAR_VERSIONS: &[(&str, &str)] = &[\n");
    for (language, tag) in rows {
        table.push_str(&format!("    ({language:?}, {tag:?}),\n"));
    }
    table.push_str("];\n");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("grammar_versions.rs"), table).expect("write grammar_versions.rs");
}
