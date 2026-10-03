// astria-detect: file discovery, classification, and incremental detection

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use astria_core::FileType;
use astria_paths::normalize;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub file_type: FileType,
    pub language: Option<String>,
    pub content_hash: String,
    pub size_bytes: u64,
}

#[derive(Debug)]
pub struct DetectResult {
    pub new: Vec<FileEntry>,
    pub changed: Vec<FileEntry>,
    pub unchanged: Vec<FileEntry>,
    pub removed: Vec<FileEntry>,
}

const DOC_EXTENSIONS: &[&str] = &[
    ".md", ".mdx", ".qmd", ".txt", ".rst", ".html", ".htm", ".yaml", ".yml", ".docx", ".xlsx",
    ".gdoc", ".gsheet", ".gslides",
];
const PAPER_EXTENSIONS: &[&str] = &[".pdf"];
const IMAGE_EXTENSIONS: &[&str] = &[".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"];
// Video/audio extension lists live in astria-core: detect classifies them
// and the astria-extract transcription route consumes the same lists.

pub fn classify_file(path: &Path) -> Option<FileType> {
    // Manifests first: `go.mod` has a `.mod` extension, the rest `.toml`/
    // `.json`/`.xml` — none of which map to a language otherwise.
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| astria_core::MANIFEST_FILENAMES.contains(&n.to_lowercase().as_str()))
        .unwrap_or(false)
    {
        return Some(FileType::Code);
    }
    // Minified vendor bundles (vis-network.min.js, vendored .min.css) are
    // extraction noise: millions of meaningless symbols that dominate the
    // god-node ranking.
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_lowercase();
        if lower.ends_with(".min.js") || lower.ends_with(".min.mjs") || lower.ends_with(".min.css")
        {
            return None;
        }
    }
    let ext = path.extension()?.to_str()?.to_lowercase();
    let ext_with_dot = format!(".{}", ext);
    if astria_core::languages::for_extension(&ext).is_some() {
        return Some(FileType::Code);
    }
    if DOC_EXTENSIONS.contains(&ext_with_dot.as_str()) {
        return Some(FileType::Document);
    }
    if PAPER_EXTENSIONS.contains(&ext_with_dot.as_str()) {
        return Some(FileType::Paper);
    }
    if IMAGE_EXTENSIONS.contains(&ext_with_dot.as_str()) {
        return Some(FileType::Image);
    }
    if astria_core::VIDEO_EXTENSIONS.contains(&ext_with_dot.as_str()) {
        return Some(FileType::Video);
    }
    if astria_core::AUDIO_EXTENSIONS.contains(&ext_with_dot.as_str()) {
        return Some(FileType::Audio);
    }
    None
}

pub fn language_for_extension(ext: &str) -> Option<&'static str> {
    astria_core::languages::for_extension(ext).map(|language| language.name)
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    // Versioned with the extraction scheme tag so scheme changes turn every
    // file "changed" once and force a clean full rebuild on upgrade.
    hasher.update(astria_core::EXTRACTION_HASH_VERSION.as_bytes());
    hasher.update([0u8]);
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn detect(root: &Path, db: &Connection) -> astria_core::Result<DetectResult> {
    let mut new_files = Vec::new();
    let mut changed_files = Vec::new();
    let mut unchanged_files = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();

    let mut ignore_builder = ignore::WalkBuilder::new(root);
    ignore_builder
        .hidden(false)
        .git_ignore(true)
        .add_custom_ignore_filename(".astriaignore");

    let astriaignore = root.join(".astriaignore");
    if astriaignore.exists() {
        // A read/parse failure must not silently proceed: the user's explicit
        // exclusions would not apply and the graph would contain files they
        // told astria to ignore. `add_ignore` returns Some(error) on failure
        // and parses the file eagerly, so malformed patterns are caught here.
        if let Some(error) = ignore_builder.add_ignore(&astriaignore) {
            return Err(astria_core::AstriaError::Graph(format!(
                "failed to load {}: {error}",
                astriaignore.display()
            )));
        }
    }

    for entry in ignore_builder.build() {
        let entry = entry.map_err(|error| {
            astria_core::AstriaError::Graph(format!("file discovery failed: {error}"))
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(path);
        let rel_str = normalize(relative);
        if rel_str.starts_with(".astria/") {
            continue;
        }
        // Never ingest secrets or unknown hidden directories — content can
        // end up in exports and LLM API requests. Excluded files fall out
        // of seen_paths, so previously ingested ones are cleaned up.
        if astria_core::security::is_sensitive_path(&rel_str) {
            continue;
        }
        // Skip build output, vendored dependencies, and bundler artifacts —
        // minified blobs flood the graph with one-letter function nodes.
        if astria_core::security::is_noise_path(&rel_str) {
            continue;
        }
        let Some(file_type) = classify_file(path) else {
            continue;
        };

        let metadata = std::fs::metadata(path)?;
        let size_bytes = metadata.len();

        // Skip files that are too large
        if !astria_core::security::check_file_size(path, size_bytes) {
            continue;
        }

        let bytes = std::fs::read(path)?;
        // Minified/generated blobs (bundler output = few, huge lines) are
        // noise: they spawn single-letter function nodes that flood hubs
        // and query results.
        let sample_len = bytes.len().min(64 * 1024);
        let sample_newlines = bytes[..sample_len].iter().filter(|&&b| b == b'\n').count();
        if astria_core::security::looks_minified(bytes.len() as u64, sample_len, sample_newlines) {
            continue;
        }

        seen_paths.insert(rel_str.clone());
        let hash = hash_bytes(&bytes);

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let language = language_for_extension(ext).map(|s| s.to_string());

        let stored_hash: Option<String> = db
            .query_row(
                "SELECT content_hash FROM file_manifest WHERE file_path = ?1",
                rusqlite::params![rel_str],
                |row| row.get(0),
            )
            .ok();

        let entry = FileEntry {
            path: relative.to_path_buf(),
            file_type,
            language,
            content_hash: hash,
            size_bytes,
        };

        match stored_hash {
            None => new_files.push(entry),
            Some(h) if h != entry.content_hash => changed_files.push(entry),
            Some(_) => unchanged_files.push(entry),
        }
    }

    // Find removed files
    let mut removed_files = Vec::new();
    let mut stmt = db.prepare(
        "SELECT file_path, content_hash, file_type, language, size_bytes FROM file_manifest",
    )?;
    let rows: Vec<(String, String, String, Option<String>, u64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (fp, hash, ft, lang, size) in rows {
        if !seen_paths.contains(&fp) {
            removed_files.push(FileEntry {
                path: PathBuf::from(&fp),
                file_type: FileType::from_str(&ft).unwrap_or(FileType::Code),
                language: lang,
                content_hash: hash,
                size_bytes: size,
            });
        }
    }

    Ok(DetectResult {
        new: new_files,
        changed: changed_files,
        unchanged: unchanged_files,
        removed: removed_files,
    })
}

pub fn update_manifest(result: &DetectResult, db: &Connection) -> astria_core::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    let all_entries: Vec<&FileEntry> = result
        .new
        .iter()
        .chain(result.changed.iter())
        .chain(result.unchanged.iter())
        .collect();
    for entry in &all_entries {
        db.execute(
            "INSERT OR REPLACE INTO file_manifest (file_path, content_hash, file_type, language, last_seen_at, size_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                normalize(&entry.path),
                entry.content_hash,
                entry.file_type.as_str(),
                entry.language,
                now,
                entry.size_bytes,
            ],
        )?;
    }
    for entry in &result.removed {
        db.execute(
            "DELETE FROM file_manifest WHERE file_path = ?1",
            rusqlite::params![normalize(&entry.path)],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;
    use std::fs;

    #[test]
    fn classify_known_extensions() {
        assert_eq!(classify_file(Path::new("foo.py")), Some(FileType::Code));
        assert_eq!(classify_file(Path::new("foo.rs")), Some(FileType::Code));
        assert_eq!(classify_file(Path::new("foo.md")), Some(FileType::Document));
        assert_eq!(
            classify_file(Path::new("foo.qmd")),
            Some(FileType::Document)
        );
        assert_eq!(
            classify_file(Path::new("foo.html")),
            Some(FileType::Document)
        );
        assert_eq!(
            classify_file(Path::new("foo.yaml")),
            Some(FileType::Document)
        );
        assert_eq!(
            classify_file(Path::new("foo.yml")),
            Some(FileType::Document)
        );
        assert_eq!(
            classify_file(Path::new("foo.docx")),
            Some(FileType::Document)
        );
        assert_eq!(
            classify_file(Path::new("foo.xlsx")),
            Some(FileType::Document)
        );
        assert_eq!(
            classify_file(Path::new("report.gdoc")),
            Some(FileType::Document)
        );
        assert_eq!(
            classify_file(Path::new("budget.gsheet")),
            Some(FileType::Document)
        );
        assert_eq!(
            classify_file(Path::new("deck.gslides")),
            Some(FileType::Document)
        );
        assert_eq!(classify_file(Path::new("foo.pdf")), Some(FileType::Paper));
        assert_eq!(classify_file(Path::new("foo.png")), Some(FileType::Image));
        assert_eq!(classify_file(Path::new("foo.mp4")), Some(FileType::Video));
        assert_eq!(classify_file(Path::new("foo.mp3")), Some(FileType::Audio));
        assert_eq!(classify_file(Path::new("foo.wav")), Some(FileType::Audio));
        assert_eq!(classify_file(Path::new("foo.xyz")), None);
        assert_eq!(classify_file(Path::new("Makefile")), None);
    }

    #[test]
    fn language_for_ext() {
        assert_eq!(language_for_extension(".py"), Some("Python"));
        assert_eq!(language_for_extension(".rs"), Some("Rust"));
        assert_eq!(language_for_extension(".xyz"), None);
    }

    #[test]
    fn detect_new_files_in_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.py"), "def hello(): pass\n").unwrap();
        fs::write(dir.path().join("readme.md"), "# Hello\n").unwrap();

        let db = open_db_in_memory().unwrap();
        let result = detect(dir.path(), &db).unwrap();

        assert_eq!(result.new.len(), 2);
        assert_eq!(result.changed.len(), 0);
        assert_eq!(result.removed.len(), 0);
        assert!(result
            .new
            .iter()
            .any(|f| f.path.to_string_lossy().contains("main.py")));
        assert!(result
            .new
            .iter()
            .any(|f| f.language.as_deref() == Some("Python")));
    }

    #[test]
    fn detect_changed_files_after_update() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.py"), "def hello(): pass\n").unwrap();

        let db = open_db_in_memory().unwrap();
        let result = detect(dir.path(), &db).unwrap();
        update_manifest(&result, &db).unwrap();

        fs::write(dir.path().join("main.py"), "def goodbye(): pass\n").unwrap();
        let result2 = detect(dir.path(), &db).unwrap();

        assert_eq!(result2.new.len(), 0);
        assert_eq!(result2.changed.len(), 1);
    }

    #[test]
    fn detect_removed_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.py"), "a\n").unwrap();
        fs::write(dir.path().join("b.py"), "b\n").unwrap();

        let db = open_db_in_memory().unwrap();
        let result = detect(dir.path(), &db).unwrap();
        update_manifest(&result, &db).unwrap();

        fs::remove_file(dir.path().join("b.py")).unwrap();
        let result2 = detect(dir.path(), &db).unwrap();

        assert_eq!(result2.removed.len(), 1);
        assert!(result2.removed[0].path.to_string_lossy().contains("b.py"));
    }

    #[test]
    fn detect_respects_astriaignore() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("keep.py"), "a\n").unwrap();
        fs::write(dir.path().join("skip.py"), "b\n").unwrap();
        fs::write(dir.path().join(".astriaignore"), "skip.py\n").unwrap();

        let db = open_db_in_memory().unwrap();
        let result = detect(dir.path(), &db).unwrap();

        assert!(result
            .new
            .iter()
            .any(|f| f.path.to_string_lossy().contains("keep.py")));
        assert!(!result
            .new
            .iter()
            .any(|f| f.path.to_string_lossy().contains("skip.py")));
    }

    #[test]
    fn detect_fails_loudly_when_astriaignore_cannot_be_loaded() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.py"), "def hello(): pass\n").unwrap();
        // A directory where the ignore file should be makes `add_ignore`
        // fail deterministically on every platform.
        fs::create_dir(dir.path().join(".astriaignore")).unwrap();

        let db = open_db_in_memory().unwrap();
        let result = detect(dir.path(), &db);

        let error = result.expect_err(
            "a .astriaignore that cannot be loaded must fail the run instead of silently proceeding without the user's exclusions",
        );
        assert!(error.to_string().contains(".astriaignore"));
    }
}
