use std::path::{Path, PathBuf};

/// Normalize a path to forward-slash representation for DB storage.
/// Converts all backslashes to forward slashes on any platform and strips
/// the Windows verbatim prefix (`\\?\C:\...` → `C:/...`), which otherwise
/// leaks into every stored path and downstream agent-facing output.
pub fn normalize(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    match s.strip_prefix("//?/") {
        Some(stripped) => stripped.to_string(),
        None => s,
    }
}

/// Display form of a stored path: relative to `root` when it lives under
/// the project root, unchanged otherwise. This keeps agent-facing output
/// (query/explain/affected lines, MCP text) short instead of repeating the
/// absolute project prefix on every line.
pub fn relative_display(path: &str, root: &str) -> String {
    let path = path.trim_start_matches("//?/");
    let root = root.trim_end_matches('/');
    let root = root.trim_start_matches("//?/");
    if root.is_empty() {
        return path.to_string();
    }
    match path.strip_prefix(root) {
        Some(rest) if rest.starts_with('/') => rest[1..].to_string(),
        _ => path.to_string(),
    }
}

/// Return the `.astria` directory path under the given root.
/// Creates the directory if it does not exist.
pub fn astria_dir(root: &Path) -> std::io::Result<PathBuf> {
    let dir = root.join(".astria");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Return the SQLite database path: `root/.astria/db.sqlite`.
/// Creates the `.astria` directory if it does not exist.
pub fn db_path(root: &Path) -> std::io::Result<PathBuf> {
    let dir = astria_dir(root)?;
    Ok(dir.join("db.sqlite"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn normalize_unix_path_unchanged() {
        assert_eq!(normalize(Path::new("src/main.rs")), "src/main.rs");
    }

    #[test]
    fn normalize_converts_backslashes() {
        assert_eq!(normalize(Path::new("src\\main.rs")), "src/main.rs");
    }

    #[test]
    fn normalize_mixed_separators() {
        assert_eq!(normalize(Path::new("src\\lib/mod.rs")), "src/lib/mod.rs");
    }

    #[test]
    fn normalize_already_normalized() {
        assert_eq!(normalize(Path::new("a/b/c.py")), "a/b/c.py");
    }

    #[test]
    fn normalize_strips_windows_verbatim_prefix() {
        assert_eq!(
            normalize(Path::new("\\\\?\\C:\\repo\\src\\lib.rs")),
            "C:/repo/src/lib.rs"
        );
    }

    #[test]
    fn relative_display_strips_root_prefix() {
        assert_eq!(
            relative_display("C:/repo/src/lib.rs", "C:/repo"),
            "src/lib.rs"
        );
    }

    #[test]
    fn relative_display_handles_verbatim_prefixes() {
        assert_eq!(
            relative_display("//?/C:/repo/src/lib.rs", "//?/C:/repo"),
            "src/lib.rs"
        );
    }

    #[test]
    fn relative_display_leaves_foreign_paths_untouched() {
        assert_eq!(
            relative_display("D:/other/main.py", "C:/repo"),
            "D:/other/main.py"
        );
        assert_eq!(relative_display("src/main.rs", ""), "src/main.rs");
    }

    #[test]
    fn relative_display_keeps_partial_prefixes() {
        // "C:/repository" must not be treated as under "C:/repo"
        assert_eq!(
            relative_display("C:/repository/src/lib.rs", "C:/repo"),
            "C:/repository/src/lib.rs"
        );
    }

    #[test]
    fn astria_dir_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let gf = astria_dir(dir.path()).unwrap();
        assert!(gf.exists());
        assert!(gf.to_string_lossy().contains(".astria"));
    }

    #[test]
    fn db_path_is_inside_astria_dir() {
        let dir = tempfile::tempdir().unwrap();
        let db = db_path(dir.path()).unwrap();
        assert!(db.to_string_lossy().contains(".astria"));
        assert!(db.to_string_lossy().contains("db.sqlite"));
    }
}
