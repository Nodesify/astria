//! Source freshness uses the same discovery and content hashes as indexing.
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub fn artifacts_match(directory: &Path, generation: &str) -> bool {
    (|| -> Option<bool> {
        let sidecar = std::fs::read_to_string(directory.join("generation.txt")).ok()?;
        let report = std::fs::read_to_string(directory.join("graph_report.md")).ok()?;
        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(directory.join("graph.json")).ok()?).ok()?;
        Some(sidecar.trim() == generation
            && report.lines().rev().find(|line| line.starts_with("generation: "))
                == Some(format!("generation: {generation}").as_str())
            && json.get("_meta")?.get("graph_generation")?.as_str()? == generation)
    })().unwrap_or(false)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphFreshness {
    pub status: String,
    pub added: usize,
    pub modified: usize,
    pub deleted: usize,
    pub files_checked: usize,
    pub extraction_outdated: bool,
    pub artifacts_checked: bool,
    pub artifacts_consistent: Option<bool>,
    pub graph_generation: Option<String>,
    pub graph_built_at: Option<String>,
    pub stale_external_indexes: Vec<String>,
    pub check_milliseconds: u64,
    pub error: Option<String>,
}

pub fn inspect(db: &Connection, root: &Path, artifacts: bool) -> GraphFreshness {
    let started = std::time::Instant::now();
    let meta = |key: &str| -> Option<String> {
        db.query_row("SELECT value FROM _meta WHERE key = ?1", [key], |r| r.get(0)).ok()
    };
    let mut result = GraphFreshness {
        status: "unknown".into(),
        graph_generation: meta("graph_generation"),
        graph_built_at: meta("graph_published_at"),
        extraction_outdated: meta("extraction_hash_version").as_deref()
            != Some(astria_core::EXTRACTION_HASH_VERSION),
        stale_external_indexes: meta("external_indexes_stale")
            .and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
        ..Default::default()
    };
    match super::detect(root, db) {
        Ok(detected) => {
            result.added = detected.new.len();
            result.modified = detected.changed.len();
            result.deleted = detected.removed.len();
            result.files_checked = detected.new.len() + detected.changed.len() + detected.unchanged.len();
            if result.graph_generation.is_some() && result.graph_built_at.is_some() {
                result.status = if result.added + result.modified + result.deleted > 0
                    || result.extraction_outdated || !result.stale_external_indexes.is_empty()
                { "stale" } else { "fresh" }.into();
            }
        }
        Err(error) => result.error = Some(error.to_string()),
    }
    if artifacts {
        result.artifacts_checked = true;
        let directory = root.join(".astria");
        let consistent = result.graph_generation.as_deref()
            .is_some_and(|generation| artifacts_match(&directory, generation));
        result.artifacts_consistent = Some(consistent);
        if !consistent && result.error.is_none() { result.status = "incomplete".into(); }
    }
    result.check_milliseconds = started.elapsed().as_millis() as u64;
    result
}
