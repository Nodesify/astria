//! File lifecycle ownership: collect the full corpus and publish its facts and
//! successful manifest together. Extraction stays incremental through its cache.
use std::path::{Path, PathBuf};

use astria_core::Result;
use astria_detect::{DetectResult, FileEntry};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

pub(super) fn detect(root: &Path, db: &Connection) -> Result<DetectResult> {
    let mut detected = astria_detect::detect(root, db)?;
    // The normal walker intentionally excludes .astria. Sidecars nevertheless
    // need the same manifest/removal lifecycle as ordinary documents.
    let transcripts = root.join(".astria/transcripts");
    if transcripts.is_dir() {
        for entry in std::fs::read_dir(transcripts)? {
            let path = entry?.path();
            if !path.is_file()
                || !matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("txt" | "md")
                )
            {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            if !astria_core::check_file_size(&path, bytes.len() as u64) {
                continue;
            }
            let mut hash = Sha256::new();
            hash.update(astria_core::EXTRACTION_HASH_VERSION.as_bytes());
            hash.update([0]);
            hash.update(&bytes);
            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            let stored = detected
                .removed
                .iter()
                .position(|e| e.path == relative)
                .map(|index| detected.removed.remove(index));
            let entry = FileEntry {
                path: relative,
                file_type: astria_core::FileType::Document,
                language: None,
                content_hash: format!("{:x}", hash.finalize()),
                size_bytes: bytes.len() as u64,
            };
            match stored {
                None => detected.new.push(entry),
                Some(old) if old.content_hash == entry.content_hash => {
                    detected.unchanged.push(entry)
                }
                Some(_) => detected.changed.push(entry),
            }
        }
    }
    Ok(detected)
}

pub(super) fn corpus(root: &Path, detected: &DetectResult) -> Vec<PathBuf> {
    let mut files: Vec<_> = detected
        .new
        .iter()
        .chain(&detected.changed)
        .chain(&detected.unchanged)
        .map(|entry| root.join(&entry.path))
        .collect();
    files.sort();
    files
}

pub(super) fn publish(
    root: &Path,
    db: &Connection,
    detected: &DetectResult,
    extractions: &[astria_extract::Extraction],
    build_configuration: &str,
    cli_version: Option<&str>,
) -> Result<astria_build::BuildResult> {
    let tx = db.unchecked_transaction()?;
    let mut replacements = extractions.to_vec();
    for entry in &detected.removed {
        let path = astria_paths::normalize(&root.join(&entry.path));
        tx.execute("DELETE FROM edges WHERE source_file = ?1", [&path])?;
        replacements.push(astria_extract::Extraction {
            file_path: root.join(&entry.path),
            language: "removed".into(),
            nodes: Vec::new(),
            edges: Vec::new(),
        });
        for key in [
            path.clone(),
            format!("semantic:{path}"),
            format!("deep:{path}"),
        ] {
            tx.execute("DELETE FROM extraction_cache WHERE file_path = ?1", [key])?;
        }
    }
    let result = astria_build::build_in_transaction(&replacements, &tx)?;
    // Deep links depend on the corpus, not just surviving symbol IDs. They
    // are restored from their validated cache only when --deep is requested.
    tx.execute("DELETE FROM edges WHERE context = 'deep'", [])?;
    astria_detect::update_manifest(detected, &tx)?;
    tx.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('build_configuration', ?1)",
        [build_configuration],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('graph_published_at', ?1)",
        [super::timestamp()],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('pipeline_version', ?1)",
        [env!("CARGO_PKG_VERSION")],
    )?;
    // The npm CLI version is the one users upgrade; it is passed in by the
    // driver because only the TS package knows it. Absent means an internal
    // caller (tests, global merge) ran the pipeline without one.
    if let Some(version) = cli_version {
        tx.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('astria_version', ?1)",
            [version],
        )?;
    }
    // Which extraction rules produced this graph; a mismatch with the
    // running binary's constant means the graph predates current rules.
    tx.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('extraction_hash_version', ?1)",
        [astria_core::EXTRACTION_HASH_VERSION],
    )?;
    tx.commit()?;
    Ok(result)
}
