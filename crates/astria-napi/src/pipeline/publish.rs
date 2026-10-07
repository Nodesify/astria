//! Publication: generation stamps, atomic artifact writes, and the
//! artifact flip that publishes graph.json + report + stamps together.
use super::*;

pub(crate) fn write_report(
    astria_dir: &Path,
    report: &str,
    stamp: &str,
) -> astria_core::Result<()> {
    // The generation rides in a footer so a consumer can verify the report,
    // graph.json, and database all describe the same publication.
    let body = format!("{report}\n---\n\ngeneration: {stamp}\n");
    write_artifact_atomic(&astria_dir.join("graph_report.md"), body.as_bytes())
}

/// Write a published artifact whole: temp sibling + rename, so a concurrent
/// reader never observes a torn or half-written file.
pub(crate) fn write_artifact_atomic(path: &Path, bytes: &[u8]) -> astria_core::Result<()> {
    let tmp = path.with_extension("new");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Fresh publication stamp: node count + wall-clock millis. Every writer
/// (pipeline, merge, global) mints one at publication so snapshot caches
/// keyed on it invalidate.
pub(crate) fn generation_stamp(db: &Connection) -> String {
    format!(
        "{}:{}",
        db.query_row(
            "SELECT COUNT(*) + COALESCE((SELECT MAX(id) FROM pipeline_runs), 0) FROM nodes",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap_or(0),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    )
}

/// Record the generation this publication produced, in the database and in
/// a sidecar stamp file next to the artifacts. Returns the stamp.
///
/// `mint` — when the run mutated the graph, a fresh stamp is minted; when
/// nothing changed, the existing generation is reused so no-op runs leave
/// the publication identity (and every cache keyed on it) stable. A missing
/// stamp is always minted.
pub(crate) fn stamp_generation(
    db: &Connection,
    astria_dir: &Path,
    mint: bool,
) -> astria_core::Result<String> {
    let stamp = if mint {
        generation_stamp(db)
    } else {
        db.query_row(
            "SELECT value FROM _meta WHERE key = 'graph_generation'",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| generation_stamp(db))
    };
    db.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('graph_generation', ?1)",
        [&stamp],
    )?;
    write_artifact_atomic(&astria_dir.join("generation.txt"), stamp.as_bytes())?;
    Ok(stamp)
}

/// Advance the publication generation immediately after a stage committed a
/// real content change. The terminal stamp covers the happy path; this
/// covers the failure path — if a later stage errors after a mutation
/// committed, snapshot caches keyed on the generation must not keep serving
/// the previous state. Mid-run stages run in autocommit, so the advance must
/// follow each mutation, not wait for publication.
pub(crate) fn advance_generation(db: &Connection) -> astria_core::Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('graph_generation', ?1)",
        [&generation_stamp(db)],
    )?;
    Ok(())
}

/// Current community assignment of every node — the before/after comparison
/// that decides whether a clustering pass actually changed the graph.
pub(crate) fn community_assignments(db: &Connection) -> HashMap<String, Option<i64>> {
    let mut map = HashMap::new();
    if let Ok(mut stmt) = db.prepare("SELECT id, community FROM nodes") {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?))
        }) {
            for row in rows.flatten() {
                map.insert(row.0, row.1);
            }
        }
    }
    map
}

/// One publication workflow for the derived artifacts: terminal generation
/// stamp, report footer, and graph.json all move together, carrying the same
/// generation. Shared by the pipeline and cluster-only republication so no
/// surface can drift from the database state it describes.
pub(crate) fn publish_artifacts(
    db: &Connection,
    astria_dir: &Path,
    report: &str,
    mint_generation: bool,
) -> astria_core::Result<String> {
    let stamp = stamp_generation(db, astria_dir, mint_generation)?;
    write_report(astria_dir, report, &stamp)?;
    export_json(db, &astria_dir.join("graph.json"))?;
    Ok(stamp)
}

/// Unchanged workspace shortcuts whose remote revision moved since the last
/// extraction. Freshness = some extraction_cache row for the path carries
/// the CURRENT revision in its fingerprint (`:gws-rev:<rev>` suffix).
pub(crate) fn stale_workspace_shortcuts(
    root: &Path,
    db: &Connection,
    unchanged: &[astria_detect::FileEntry],
) -> usize {
    let mut stale = 0usize;
    for entry in unchanged {
        let is_shortcut = entry
            .path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_lowercase().as_str(), "gdoc" | "gsheet" | "gslides"));
        if !is_shortcut {
            continue;
        }
        let path = root.join(&entry.path);
        let Some(rev) = astria_gws::remote_revision(&path) else {
            continue; // offline / no credentials: local hash stands
        };
        let key = astria_paths::normalize(&path);
        let fresh = db
            .prepare("SELECT content_hash FROM extraction_cache WHERE file_path = ?1")
            .and_then(|mut stmt| {
                let hashes: Vec<String> = stmt
                    .query_map(rusqlite::params![key], |r| r.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(hashes
                    .iter()
                    .any(|h| h.ends_with(&format!(":gws-rev:{rev}"))))
            })
            .unwrap_or(false);
        if !fresh {
            stale += 1;
        }
    }
    stale
}
