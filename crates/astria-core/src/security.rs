use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

/// Redact recognizable credentials without treating ordinary hashes or identifiers as secrets.
/// Call on the complete input before truncating, persisting, or sending it to a model.
pub fn redact_secrets(text: &str) -> String {
    static PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
        [
            r"(?s)-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----.*?(?:-----END (?:[A-Z0-9]+ )*PRIVATE KEY-----|$)",
            r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b",
            // `(?:\b|_)` so a token glued into a longer identifier by an
            // underscore (`token_ghp_...`) is still caught — underscore is a
            // word character, so a plain \b never fires inside such glue.
            r"(?:\b|_)(?:sk-[A-Za-z0-9_-]{12,}|(?:ghp_|gho_|ghu_|ghs_|ghr_|github_pat_)[A-Za-z0-9_]{12,}|glpat-[A-Za-z0-9_-]{12,}|xox[baprse]-[A-Za-z0-9-]{12,}|AIza[A-Za-z0-9_-]{20,}|(?:AKIA|ASIA)[A-Z0-9]{16}|(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{12,}|npm_[A-Za-z0-9]{12,}|hf_[A-Za-z0-9]{16,}|pypi-[A-Za-z0-9_-]{16,}|lin_api_[A-Za-z0-9]{16,}|dop_v1_[A-Za-z0-9]{16,})\b",
        ].iter().map(|pattern| regex::Regex::new(pattern).expect("valid secret pattern")).collect()
    });
    static BEARER: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?i)\bBearer[ \t]+["']?([^\s"'`<>,;]+)["']?"#)
            .expect("valid Bearer pattern")
    });
    static URL_PASSWORD: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?i)([a-z][a-z0-9+.-]*://[^\s/@:"']+:)[^\s/@"']+(@)"#)
            .expect("valid URL password pattern")
    });
    static URL_EMPTY_USER: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?i)([a-z][a-z0-9+.-]*://:)[^\s/@"']+(@)"#)
            .expect("valid empty-user URL password pattern")
    });
    let mut redacted = text.to_owned();
    for pattern in PATTERNS.iter() {
        redacted = pattern.replace_all(&redacted, "[REDACTED]").into_owned();
    }
    redacted = BEARER
        .replace_all(&redacted, |captures: &regex::Captures<'_>| {
            // Common prose about the scheme is not itself a credential.
            if matches!(
                captures[1].to_ascii_lowercase().as_str(),
                "authentication"
                    | "authorization"
                    | "credentials"
                    | "credential"
                    | "scheme"
                    | "token"
                    | "tokens"
            ) {
                captures[0].to_owned()
            } else {
                "Bearer [REDACTED]".to_owned()
            }
        })
        .into_owned();
    redacted = URL_PASSWORD
        .replace_all(&redacted, "${1}[REDACTED]${2}")
        .into_owned();
    URL_EMPTY_USER
        .replace_all(&redacted, "${1}[REDACTED]${2}")
        .into_owned()
}

const MAX_TEXT_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
const MAX_BINARY_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50 MB
const MAX_LABEL_LEN: usize = 256;
const MAX_DOCSTRING_LEN: usize = 4096;

/// Validate that a file path stays within the project root.
pub fn validate_path(root: &Path, file_path: &Path) -> Result<(), String> {
    let canonical_root = root
        .canonicalize()
        .map_err(|e| format!("invalid root: {}", e))?;
    let parent = file_path.parent().unwrap_or(file_path);
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("invalid path: {}", e))?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(format!(
            "path escapes project root: {}",
            file_path.display()
        ));
    }
    Ok(())
}

/// Resolve a relative path read from repository CONTENT — a workspace member
/// list, a `path = "..."` dependency, a package `main` field — against
/// `base`, returning it only when it stays inside `root`.
///
/// Purely lexical: nothing here lets the operating system see the path
/// before it has been checked. A repository is untrusted input, and on
/// Windows an attacker-chosen UNC spelling (`\\host\share\x`) makes the OS
/// authenticate to that host, leaking an NTLM response with no code run.
/// Absolute, rooted, drive, UNC and verbatim spellings are refused outright,
/// and `..` may not climb past `root`.
pub fn confine_to_root(base: &Path, relative: &str, root: &Path) -> Option<PathBuf> {
    if relative.trim().is_empty() {
        return None;
    }
    // Both separators are separators on every platform, so a backslash
    // spelling cannot slip through a Unix host or CI run. A drive letter is
    // judged the same way: Unix has no prefix to recognise, so `C:\x` and
    // the drive-relative `C:x` are refused here rather than taken for a
    // directory named `C:`.
    let normalized = relative.replace('\\', "/");
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return None;
    }
    let mut stack: Vec<std::ffi::OsString> = base
        .components()
        .map(|component| component.as_os_str().to_os_string())
        .collect();
    // `..` may consume real components, never the prefix or root they hang
    // from.
    let anchor = base
        .components()
        .take_while(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
        .count();
    for component in Path::new(&normalized).components() {
        match component {
            Component::Prefix(_) | Component::RootDir => return None,
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.len() <= anchor {
                    return None;
                }
                stack.pop();
            }
            Component::Normal(part) => stack.push(part.to_os_string()),
        }
    }
    let joined: PathBuf = stack.into_iter().collect();
    // A leading `.` is spelling, not structure: `./repo/x` and `repo/x`
    // name the same place.
    fn without_cur_dir(path: &Path) -> PathBuf {
        path.components()
            .filter(|component| !matches!(component, Component::CurDir))
            .collect()
    }
    let inside = without_cur_dir(&joined).starts_with(without_cur_dir(root));
    inside.then_some(joined)
}

/// Check if a file is within acceptable size limits for processing.
pub fn check_file_size(path: &Path, size_bytes: u64) -> bool {
    if size_bytes > MAX_BINARY_FILE_SIZE {
        return false;
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let text_extensions = [
        "py", "js", "jsx", "mjs", "ts", "tsx", "rs", "go", "java", "c", "h", "cpp", "cc", "cxx",
        "hpp", "md", "mdx", "txt", "rst", "toml", "yaml", "yml", "json", "xml", "rb", "swift",
        "kt", "scala", "php", "cs", "lua", "hs", "ex", "sh", "bash", "dart", "zig", "css", "html",
        "sql",
    ];
    let ext_lower = ext.to_lowercase();
    if text_extensions.contains(&ext_lower.as_str()) && size_bytes > MAX_TEXT_FILE_SIZE {
        return false;
    }
    true
}

/// Sanitize a node label: strip control chars and cap length.
pub fn sanitize_label(label: &str) -> String {
    let cleaned: String = redact_secrets(label)
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(MAX_LABEL_LEN)
        .collect();
    cleaned
}

/// Sanitize a docstring: strip control chars and cap length.
pub fn sanitize_docstring(doc: &str) -> String {
    let cleaned: String = redact_secrets(doc)
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(MAX_DOCSTRING_LEN)
        .collect();
    cleaned
}

/// Hidden directories that hold build/CI config worth graphing; every other
/// dot-directory is skipped.
const ALLOWED_HIDDEN_DIRS: &[&str] = &[".github", ".gitlab", ".circleci", ".config", ".husky"];

/// File extensions that should never enter the graph (keys, certificates,
/// keystores). `env` covers environment files without the leading dot
/// (`config/production.env`); the dot-prefixed forms are matched by name
/// below. `tfstate`/`tfvars` are infrastructure state and variables, which
/// hold secrets in plain text. `p8`/`pk8` are PKCS#8 private keys, `gpg`/
/// `pgp` key rings, `keytab` a Kerberos key tab.
const SENSITIVE_EXTENSIONS: &[&str] = &[
    "pem", "key", "p12", "pfx", "jks", "keystore", "kdbx", "ppk", "p8", "pk8", "gpg", "pgp",
    "keytab", "env", "tfstate", "tfvars",
];

/// Config/data extensions for which secret-looking *names* are blocked.
/// Source-code files are exempt so `password-validator.ts` still graphs.
const CONFIG_EXTENSIONS: &[&str] = &[
    "json",
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "xml",
    "properties",
    "txt",
];

const SENSITIVE_EXACT_NAMES: &[&str] = &[
    ".npmrc",
    ".netrc",
    "_netrc",
    ".htpasswd",
    ".htdigest",
    ".git-credentials",
    ".gitconfig",
    ".yarnrc.yml",
    ".pypirc",
    ".dockercfg",
    ".vault-token",
    ".pgpass",
    ".my.cnf",
    ".ldaprc",
    "id_rsa",
    "id_dsa",
    "id_ed25519",
    "id_ecdsa",
    "credentials",
];

/// True when a repo-relative path should never be ingested: it lives in a
/// hidden directory we don't allowlist, or it looks like it carries
/// secrets (.env*, credentials, keys, certificates). Independent of
/// .gitignore so projects without one stay safe — ingested content can end
/// up in exports and, with an LLM backend configured, in API requests.
pub fn is_sensitive_path(rel_path: &str) -> bool {
    let normalized = rel_path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').collect();
    if parts.is_empty() {
        return false;
    }
    let file = parts[parts.len() - 1];
    let file_lower = file.to_lowercase();

    // Hidden directories: skip unless allowlisted. `.git` is never allowed.
    for dir in &parts[..parts.len() - 1] {
        if let Some(name) = dir.strip_prefix('.') {
            if name.is_empty() {
                continue;
            }
            if *dir == ".git" {
                return true;
            }
            if !ALLOWED_HIDDEN_DIRS.contains(dir) {
                return true;
            }
        }
    }

    if SENSITIVE_EXACT_NAMES.contains(&file_lower.as_str()) {
        return true;
    }
    if file_lower.starts_with(".env") {
        return true;
    }

    let ext = file_lower.rsplit('.').next().unwrap_or("");
    let has_ext = file_lower.contains('.');
    if has_ext && SENSITIVE_EXTENSIONS.contains(&ext) {
        return true;
    }
    // Secret-looking names, but only for config/data files so source code
    // like password-validator.ts is not excluded.
    if has_ext && CONFIG_EXTENSIONS.contains(&ext) {
        let stem = &file_lower[..file_lower.len() - ext.len() - 1];
        if stem.contains("secret") || stem.contains("credential") || stem.contains("password") {
            return true;
        }
    }
    false
}

/// Directory names that hold build artifacts, dependencies, or bundles —
/// graph noise rather than project source. Mostly redundant with
/// .gitignore, but keeps projects without one clean too.
pub const NOISE_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "venv",
    ".venv",
];

/// True when a repo-relative path is build output, a dependency tree, a
/// bundler artifact (`*.min.js`, sourcemaps), or lives in a noise
/// directory. Minified blobs especially pollute the graph with single-letter
/// function nodes (`g()`, `xA()`) that flood hubs and query results.
pub fn is_noise_path(rel_path: &str) -> bool {
    let normalized = rel_path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').collect();
    if parts.is_empty() {
        return false;
    }
    if parts[..parts.len() - 1]
        .iter()
        .any(|dir| NOISE_DIRS.contains(dir))
    {
        return true;
    }
    let file = parts[parts.len() - 1].to_lowercase();
    if file.ends_with(".min.js") || file.ends_with(".min.css") || file.ends_with(".min.mjs") {
        return true;
    }
    // Source maps are generated JSON blobs.
    file.ends_with(".map")
}

/// Heuristic for minified/generated content: a large file whose sampled
/// prefix has very long average lines (bundlers emit few, huge lines).
/// `sample_len`/`sample_newlines` describe the first slice of the file.
pub fn looks_minified(total_len: u64, sample_len: usize, sample_newlines: usize) -> bool {
    if total_len < 20_000 {
        return false;
    }
    if sample_len == 0 {
        return true;
    }
    if sample_newlines == 0 {
        return true;
    }
    sample_len / sample_newlines > 500
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_catches_common_token_families() {
        assert_eq!(redact_secrets("ghp_ABCDEFGHIJKLMNOPQR"), "[REDACTED]");
        assert_eq!(redact_secrets("sk-ant-api03-ABCDEFGHIJ"), "[REDACTED]");
        assert!(redact_secrets("see xoxb-1234567890abcdef").contains("[REDACTED]"));
    }

    #[test]
    fn redact_catches_underscore_glued_tokens() {
        // Underscore glue hides the word boundary; the connector itself is
        // consumed by the redaction, which is safe for labels.
        let out = redact_secrets("token_ghp_ABCDEFGHIJKLMNOPQR");
        assert!(!out.contains("ghp_ABCDEFGHIJKLMNOPQR"), "{out}");
        assert!(out.contains("[REDACTED]"), "{out}");
    }

    #[test]
    fn redact_catches_slack_refresh_and_empty_user_url() {
        // xoxe- refresh tokens join the xox family.
        assert!(redact_secrets("xoxe-1234567890abcdef").contains("[REDACTED]"));
        // https://:password@host has an empty user and no password scheme
        // match without its own pattern.
        let out = redact_secrets("https://:hunter2@example.com/path");
        assert!(!out.contains("hunter2"), "{out}");
        assert!(out.contains("example.com"), "{out}");
    }

    #[test]
    fn redact_leaves_ordinary_text_alone() {
        let prose = "the task list mentions sk-learning and normal URLs like https://example.com/a";
        assert_eq!(redact_secrets(prose), prose);
        // Prose about the Bearer scheme is not a credential.
        assert_eq!(
            redact_secrets("set the Bearer authentication header"),
            "set the Bearer authentication header"
        );
    }

    #[test]
    fn sanitize_label_strips_control_chars() {
        let input = "hello\x00world\x01!";
        assert_eq!(sanitize_label(input), "helloworld!");
    }

    #[test]
    fn sanitize_label_caps_length() {
        let input: String = "x".repeat(500);
        assert_eq!(sanitize_label(&input).len(), MAX_LABEL_LEN);
    }

    #[test]
    fn check_file_size_allows_normal_files() {
        use std::path::PathBuf;
        assert!(check_file_size(&PathBuf::from("main.py"), 1024));
        assert!(check_file_size(
            &PathBuf::from("main.py"),
            MAX_TEXT_FILE_SIZE
        ));
        assert!(!check_file_size(
            &PathBuf::from("main.py"),
            MAX_TEXT_FILE_SIZE + 1
        ));
    }

    #[test]
    fn check_file_size_allows_larger_binary() {
        use std::path::PathBuf;
        assert!(check_file_size(
            &PathBuf::from("image.png"),
            MAX_TEXT_FILE_SIZE + 1
        ));
        assert!(!check_file_size(
            &PathBuf::from("image.png"),
            MAX_BINARY_FILE_SIZE + 1
        ));
    }

    #[test]
    fn sensitive_paths_blocked() {
        assert!(is_sensitive_path(".env"));
        assert!(is_sensitive_path(".env.local"));
        assert!(is_sensitive_path("config/.env"));
        assert!(is_sensitive_path("certs/server.pem"));
        assert!(is_sensitive_path("keys/api.key"));
        assert!(is_sensitive_path(".ssh/id_rsa"));
        assert!(is_sensitive_path("secrets.yaml"));
        assert!(is_sensitive_path("deploy/credentials.json"));
        assert!(is_sensitive_path(".npmrc"));
        assert!(is_sensitive_path(".git/config"));
        // Hidden tool dirs not allowlisted are skipped too
        assert!(is_sensitive_path(".zcode/settings.json"));
        // An environment file is one with or without the leading dot.
        assert!(is_sensitive_path("config/production.env"));
        // Tool credentials with fixed names, and other private-key formats.
        assert!(is_sensitive_path("_netrc"));
        assert!(is_sensitive_path(".yarnrc.yml"));
        assert!(is_sensitive_path(".pypirc"));
        assert!(is_sensitive_path(".dockercfg"));
        assert!(is_sensitive_path(".htdigest"));
        assert!(is_sensitive_path(".vault-token"));
        assert!(is_sensitive_path("deploy/server.ppk"));
        // Infrastructure state and variables carry secrets in plain text.
        assert!(is_sensitive_path("infra/terraform.tfstate"));
        assert!(is_sensitive_path("infra/prod.tfvars"));
        // Other private-key containers, and database password files.
        assert!(is_sensitive_path("signing/platform.pk8"));
        assert!(is_sensitive_path("keys/AuthKey_ABCD1234.p8"));
        assert!(is_sensitive_path("keys/private.pgp"));
        assert!(is_sensitive_path("gnupg/secring.gpg"));
        assert!(is_sensitive_path("krb5/service.keytab"));
        assert!(is_sensitive_path(".pgpass"));
        assert!(is_sensitive_path(".my.cnf"));
        assert!(is_sensitive_path(".ldaprc"));
    }

    #[test]
    fn normal_paths_allowed() {
        assert!(!is_sensitive_path("src/main.rs"));
        assert!(!is_sensitive_path("package.json"));
        assert!(!is_sensitive_path(".github/workflows/ci.yml"));
        assert!(!is_sensitive_path("docs/readme.md"));
        // source code is exempt from secret-looking-name rules
        assert!(!is_sensitive_path("src/password_validator.ts"));
        assert!(!is_sensitive_path("src/SecretService.java"));
        // Source that merely mentions environments is not an environment file.
        assert!(!is_sensitive_path("src/env.d.ts"));
        assert!(!is_sensitive_path("src/environment.ts"));
        assert!(!is_sensitive_path("infra/main.tf"));
        // A key is a file type, not a word in a source file's name.
        assert!(!is_sensitive_path("src/api_keys.rs"));
        assert!(!is_sensitive_path("src/KeybindManager.ts"));
        assert!(!is_sensitive_path("src/pgp_utils.py"));
    }

    #[test]
    fn noise_paths_blocked() {
        assert!(is_noise_path("web/dist/app.min.js"));
        assert!(is_noise_path("crates/web/src/assets/vis-network.min.js"));
        assert!(is_noise_path("app.js.map"));
        assert!(is_noise_path("node_modules/left-pad/index.js"));
        assert!(is_noise_path("target/release/build.rs"));
    }

    #[test]
    fn source_paths_not_noise() {
        assert!(!is_noise_path("src/main.rs"));
        assert!(!is_noise_path("web/app.js"));
        assert!(!is_noise_path("docs/world.map.md"));
        assert!(!is_noise_path("src/map.ts"));
    }

    #[test]
    fn minified_heuristic_flags_bundles_not_source() {
        assert!(looks_minified(500_000, 4096, 4), "avg 1KB lines = minified");
        assert!(looks_minified(500_000, 4096, 0), "no newlines = minified");
        assert!(
            !looks_minified(90_000, 4096, 100),
            "normal source ~40 chars/line"
        );
        assert!(
            !looks_minified(5_000, 4096, 1),
            "small files are never flagged"
        );
    }

    #[test]
    fn confined_paths_resolve_inside_the_root() {
        let root = Path::new("/repo");
        let base = Path::new("/repo/crates/cli");
        let confined = |relative: &str| confine_to_root(base, relative, root);
        assert_eq!(
            confined("../core").as_deref(),
            Some(Path::new("/repo/crates/core"))
        );
        assert_eq!(
            confined("./src/lib.rs").as_deref(),
            Some(Path::new("/repo/crates/cli/src/lib.rs"))
        );
        // Climbing up to the root itself is still inside it; one more is not.
        assert_eq!(confined("../..").as_deref(), Some(Path::new("/repo")));
        assert_eq!(confined("../../.."), None);
        assert_eq!(confined(""), None);
        assert_eq!(confined("   "), None);
    }

    #[test]
    fn absolute_and_unc_spellings_are_refused_before_any_lookup() {
        let root = Path::new("/repo");
        let base = Path::new("/repo/ws");
        for spelling in [
            "\\\\host\\share\\crate",
            "//host/share/crate",
            "\\\\?\\C:\\repo\\crate",
            "C:\\elsewhere\\crate",
            // Drive-relative, and so absolute on whichever platform reads it.
            "D:relative\\crate",
            "/elsewhere/crate",
        ] {
            assert_eq!(
                confine_to_root(base, spelling, root),
                None,
                "{spelling:?} must be refused"
            );
        }
    }

    #[test]
    fn root_relative_spellings_compare_by_components() {
        let root = Path::new(".");
        assert_eq!(
            confine_to_root(Path::new("./crates/cli"), "../core", root).as_deref(),
            Some(Path::new("./crates/core"))
        );
        assert_eq!(
            confine_to_root(Path::new("crates/cli"), "../..", root).as_deref(),
            Some(Path::new(""))
        );
        assert_eq!(
            confine_to_root(Path::new("crates/cli"), "../../..", root),
            None
        );
    }
}
