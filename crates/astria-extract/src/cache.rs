// cache: extraction_cache table access, shared by every extraction path.

use std::path::Path;

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::schema::{ExtractedEdge, ExtractedNode, Extraction};
use astria_core::AstriaError;
use astria_paths::normalize;

/// SHA-256 hash of the file contents, versioned with the extraction scheme
/// tag so scheme changes (e.g. the id-format change in v2) invalidate all
/// cached extractions and force one clean re-extraction.
pub(crate) fn file_hash(path: &Path) -> Result<String, AstriaError> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(astria_core::EXTRACTION_HASH_VERSION.as_bytes());
    hasher.update([0u8]);
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Check the extraction_cache table. Returns cached Extraction if hit.
pub(crate) fn check_cache(db: &Connection, path: &Path, hash: &str) -> Option<Extraction> {
    let path_str = normalize(path);
    let mut stmt = db
        .prepare(
            "SELECT language, nodes, edges FROM extraction_cache WHERE file_path = ?1 AND content_hash = ?2",
        )
        .ok()?;
    stmt.query_row(rusqlite::params![&path_str, hash], |row| {
        let language: String = row.get(0)?;
        let nodes_json: String = row.get(1)?;
        let edges_json: String = row.get(2)?;
        Ok((language, nodes_json, edges_json))
    })
    .ok()
    .map(|(language, nodes_json, edges_json)| {
        let nodes: Vec<ExtractedNode> = serde_json::from_str(&nodes_json).unwrap_or_default();
        let edges: Vec<ExtractedEdge> = serde_json::from_str(&edges_json).unwrap_or_default();
        Extraction {
            file_path: path.to_path_buf(),
            language,
            nodes,
            edges,
        }
    })
}

/// Save extraction result to the cache table.
pub(crate) fn save_cache(db: &Connection, path: &Path, hash: &str, extraction: &Extraction) {
    let path_str = normalize(path);
    let nodes_json = serde_json::to_string(&extraction.nodes).unwrap_or_default();
    let edges_json = serde_json::to_string(&extraction.edges).unwrap_or_default();
    let now = chrono_free_timestamp();

    if let Err(e) = db.execute(
        "INSERT OR REPLACE INTO extraction_cache (file_path, content_hash, language, nodes, edges, extracted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            &path_str,
            hash,
            extraction.language,
            nodes_json,
            edges_json,
            now,
        ],
    ) {
        eprintln!("warning: failed to cache extraction for {}: {}", path_str, e);
    }
}

/// Simple timestamp without needing chrono.
fn chrono_free_timestamp() -> String {
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    )
}
