use std::path::Path;

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
    let cleaned: String = label
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(MAX_LABEL_LEN)
        .collect();
    cleaned
}

/// Sanitize a docstring: strip control chars and cap length.
pub fn sanitize_docstring(doc: &str) -> String {
    let cleaned: String = doc
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
/// keystores).
const SENSITIVE_EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx", "jks", "keystore", "kdbx"];

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
    ".htpasswd",
    ".git-credentials",
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
}
