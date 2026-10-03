use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn timestamp() -> String {
    super::timestamp()
}

/// True when a file's semantic input must come from the document layer's
/// derived text rather than a raw read: media transcripts and workspace
/// exports (identified by their extraction language) plus the binary
/// document extensions.
fn needs_derived_text(path: &Path, extractions: &[astria_extract::Extraction]) -> bool {
    if let Some(ext) = extractions
        .iter()
        .find(|e| e.file_path == path)
        .map(|e| e.language.as_str())
    {
        if matches!(ext, "transcript" | "gws") {
            return true;
        }
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_lowercase().as_str(), "pdf" | "docx" | "xlsx"))
        .unwrap_or(false)
}

/// Load the document layer's derived text for a file. Rows are keyed by the
/// same normalized path extraction wrote and by the extraction-layer content
/// hash (plain, or `:gws-rev:`-suffixed for workspace shortcuts) — NOT by
/// the semantic configuration hash, which never matches what extraction
/// stored. Stale rows (file changed since extraction) count as missing.
fn load_derived_text(db: &Connection, path: &Path) -> Option<String> {
    let plain_hash = astria_extract::cache::file_hash(path).ok()?;
    let key = astria_paths::normalize(path);
    let mut stmt = db
        .prepare("SELECT content_hash, text FROM derived_text WHERE file_path = ?1")
        .ok()?;
    let row: Option<(String, String)> = stmt
        .query_row(rusqlite::params![key], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .ok();
    let (stored_hash, text) = row?;
    astria_extract::is_extraction_hash_for(&stored_hash, &plain_hash)
        .then_some(text)
        .filter(|t: &String| !t.trim().is_empty())
}

fn semantic_cache_key(path: &Path) -> String {
    format!("semantic:{}", astria_paths::normalize(path))
}

pub(super) fn fingerprint(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

pub(super) fn configuration() -> astria_core::Result<String> {
    if !astria_semantic::enrichment_enabled() {
        return Ok("disabled".into());
    }
    let backend = astria_semantic::backend_from_env()?;
    Ok(fingerprint(&[&astria_semantic::cache_configuration(
        backend.as_ref(),
    )]))
}

fn file_hash(path: &Path, configuration: &str) -> astria_core::Result<String> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(configuration.as_bytes());
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn check_semantic_cache(
    db: &Connection,
    path: &Path,
    hash: &str,
) -> Option<astria_semantic::SemanticExtraction> {
    let key = semantic_cache_key(path);
    let mut stmt = db
        .prepare(
            "SELECT nodes, edges FROM extraction_cache WHERE file_path = ?1 AND content_hash = ?2",
        )
        .ok()?;
    stmt.query_row(rusqlite::params![&key, hash], |row| {
        let nodes_json: String = row.get(0)?;
        let edges_json: String = row.get(1)?;
        Ok((nodes_json, edges_json))
    })
    .ok()
    .and_then(|(nodes_json, edges_json)| {
        let nodes: Vec<astria_semantic::SemanticNode> = serde_json::from_str(&nodes_json).ok()?;
        let edges: Vec<astria_semantic::SemanticEdge> = serde_json::from_str(&edges_json).ok()?;
        Some(astria_semantic::SemanticExtraction { nodes, edges })
    })
}

fn save_semantic_cache(
    db: &Connection,
    path: &Path,
    hash: &str,
    extraction: &astria_semantic::SemanticExtraction,
) {
    let key = semantic_cache_key(path);
    let nodes_json = serde_json::to_string(&extraction.nodes).unwrap_or_default();
    let edges_json = serde_json::to_string(&extraction.edges).unwrap_or_default();
    let now = timestamp();
    if let Err(e) = db.execute(
        "INSERT OR REPLACE INTO extraction_cache (file_path, content_hash, language, nodes, edges, extracted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![&key, hash, "semantic", nodes_json, edges_json, now],
    ) {
        eprintln!("warning: failed to cache semantic extraction for {}: {}", key, e);
    }
}

/// Enrich existing extractions with LLM-based semantic data.
/// Disabled unless a backend was explicitly selected. Cached and fresh
/// results are merged through the same path.
/// Image files (no AST extraction) get their own synthetic extraction via
/// the backend's vision path. Cache misses are extracted in parallel by a
/// bounded worker pool (`ASTRIA_LLM_CONCURRENCY`, default 4). Returns
/// enriched/failed/cached file counts — failures are reported, never
/// silently dropped.
pub(super) struct SemanticPassStats {
    pub enriched: usize,
    pub cached: usize,
    /// Files the backend's gate dropped before extraction (not failures).
    pub gated: usize,
}

pub(super) fn enrich_with_semantics(
    files: &[PathBuf],
    extractions: &mut Vec<astria_extract::Extraction>,
    db: &Connection,
    configuration: &str,
) -> astria_core::Result<SemanticPassStats> {
    let backend_factory = || astria_semantic::backend_from_env();
    // Gate on backend availability before doing any work.
    if configuration == "disabled" {
        return Ok(SemanticPassStats {
            enriched: 0,
            cached: 0,
            gated: 0,
        });
    }
    let backend = backend_factory()?;

    let mut file_to_idx: HashMap<PathBuf, usize> = HashMap::new();
    for (i, ext) in extractions.iter().enumerate() {
        file_to_idx.insert(ext.file_path.clone(), i);
    }

    // First pass: resolve extraction slots and collect cache misses.
    struct Pending {
        path: PathBuf,
        hash: String,
        idx: usize,
    }
    let mut pending: Vec<Pending> = Vec::new();
    let mut ready: Vec<(usize, PathBuf, astria_semantic::SemanticExtraction)> = Vec::new();
    for file_path in files {
        if file_to_idx
            .get(file_path)
            .is_some_and(|&idx| extractions[idx].language == "media")
            && !astria_semantic::is_image_file(file_path)
        {
            continue;
        }
        // MCP server configs embed literal credentials and are ingested by
        // the deterministic manifest extractor (names only). Their raw
        // bytes never become semantic candidates — not for the engine, not
        // for the judge gate, not for the cache.
        if file_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(astria_core::is_mcp_config_filename)
        {
            continue;
        }
        let hash = file_hash(file_path, configuration)?;

        // Text extraction paths require an existing extraction (images have
        // none); the vision path inside extract_semantic_for_files handles
        // both kinds, so a synthetic extraction carries image results.
        let idx = match file_to_idx.get(file_path) {
            Some(&idx) => idx,
            None if astria_semantic::is_image_file(file_path) => {
                extractions.push(astria_extract::Extraction {
                    file_path: file_path.clone(),
                    language: "image".to_string(),
                    nodes: Vec::new(),
                    edges: Vec::new(),
                });
                let idx = extractions.len() - 1;
                file_to_idx.insert(file_path.clone(), idx);
                idx
            }
            None => continue,
        };
        if let Some(extraction) = check_semantic_cache(db, file_path, &hash) {
            ready.push((idx, file_path.clone(), extraction));
        } else {
            pending.push(Pending {
                path: file_path.clone(),
                hash,
                idx,
            });
        }
    }
    let cached = ready.len();

    // The backend may gate candidate files (Jev's batch keep/drop
    // judgments): files it drops never cost an engine call. The gate can
    // only save calls — gated files simply keep their AST-only extraction.
    //
    // Verdicts are cached per (content hash, backend identity): the live
    // gate model re-rolls between runs, which made judge-gated graphs
    // irreproducible (the same tree gated 31 files on one build and 17 on
    // a replay). With the cache, identical bytes plus identical judge
    // configuration reuse the previous decision — deterministic re-runs,
    // and unchanged files cost no judge calls. `off` restores live gating.
    let all_pending_paths: Vec<PathBuf> = pending.iter().map(|p| p.path.clone()).collect();
    let gate_cache_on = std::env::var("ASTRIA_LLM_JEV_GATE_CACHE")
        .map(|v| {
            !matches!(
                v.trim().to_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            )
        })
        .unwrap_or(true);
    let pending_paths: Vec<PathBuf> = if gate_cache_on {
        let gate_identity = backend.cache_identity();
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS gate_cache (
                hash TEXT NOT NULL,
                identity TEXT NOT NULL,
                keep INTEGER NOT NULL,
                judged_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (hash, identity)
            )",
        )?;
        let mut cached_kept: Vec<PathBuf> = Vec::new();
        let mut to_judge: Vec<(usize, PathBuf)> = Vec::new();
        for (pi, p) in pending.iter().enumerate() {
            let cached: Option<i64> = db
                .query_row(
                    "SELECT keep FROM gate_cache WHERE hash = ?1 AND identity = ?2",
                    rusqlite::params![p.hash, gate_identity],
                    |r| r.get(0),
                )
                .ok();
            match cached {
                Some(keep) if keep != 0 => cached_kept.push(p.path.clone()),
                Some(_) => {}
                None => to_judge.push((pi, p.path.clone())),
            }
        }
        let judge_paths: Vec<PathBuf> = to_judge.iter().map(|(_, p)| p.clone()).collect();
        let judged_kept: std::collections::HashSet<PathBuf> =
            backend.gate_files(&judge_paths).into_iter().collect();
        for (pi, path) in &to_judge {
            db.execute(
                "INSERT OR REPLACE INTO gate_cache (hash, identity, keep) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    pending[*pi].hash,
                    gate_identity,
                    judged_kept.contains(path) as i64
                ],
            )?;
        }
        let mut kept = cached_kept;
        kept.extend(judge_paths.into_iter().filter(|p| judged_kept.contains(p)));
        kept
    } else {
        backend.gate_files(&all_pending_paths)
    };
    let gated = all_pending_paths.len() - pending_paths.len();

    // F20: binary formats (office documents, workspace exports, media
    // transcripts, PDFs) already had their text extracted by the document
    // layer. Enrichment must consume that same normalized content — reading
    // the raw bytes as UTF-8 fails and aborts publication. Files whose
    // derived text is missing or stale keep their AST-only facts with a
    // warning instead of failing the run; the next rebuild re-extracts and
    // enriches them.
    let mut sources: HashMap<PathBuf, astria_semantic::SourceContent> = HashMap::new();
    let mut usable_paths: Vec<PathBuf> = Vec::with_capacity(pending_paths.len());
    for path in &pending_paths {
        if !needs_derived_text(path, extractions) {
            usable_paths.push(path.clone());
            continue;
        }
        match load_derived_text(db, path) {
            Some(text) => {
                sources.insert(
                    path.clone(),
                    astria_semantic::SourceContent::Extracted(text),
                );
                usable_paths.push(path.clone());
            }
            None => {
                eprintln!(
                    "warning: no derived text for {} — semantic enrichment skipped (AST facts kept); rebuild with `astria run` to enrich",
                    path.display()
                );
            }
        }
    }
    let pending_paths = usable_paths;

    // Batch-extract cache misses in parallel.
    let results = astria_semantic::extract_semantic_for_files_parallel_with_content(
        &pending_paths,
        backend_factory,
        std::sync::Arc::new(sources),
        astria_semantic::concurrency_from_env(),
    );
    let extraction_by_path: HashMap<PathBuf, astria_semantic::SemanticExtraction> = results
        .into_iter()
        .filter_map(|(path, result)| match result {
            Ok(extraction) => {
                let meta = pending.iter().find(|p| p.path == path);
                if let Some(meta) = meta {
                    save_semantic_cache(db, &path, &meta.hash, &extraction);
                }
                Some((path, extraction))
            }
            Err(e) => {
                eprintln!(
                    "warning: semantic extraction failed for {}: {}",
                    path.display(),
                    e
                );
                None
            }
        })
        .collect();

    let failed = pending_paths.len() - extraction_by_path.len();
    if failed > 0 {
        return Err(astria_core::AstriaError::Graph(format!(
            "semantic extraction failed for {failed} file(s); graph and manifest were not advanced; retry to reuse successful cached results"
        )));
    }
    for meta in &pending {
        let Some(sem_ext) = extraction_by_path.get(&meta.path) else {
            continue;
        };
        ready.push((meta.idx, meta.path.clone(), sem_ext.clone()));
    }
    // One merge path gives a cache hit exactly the same graph facts as a call.
    for (idx, path, sem_ext) in ready {
        let ext = &mut extractions[idx];
        for sem_node in &sem_ext.nodes {
            ext.nodes.push(astria_extract::ExtractedNode {
                id: sem_node.id.clone(),
                label: sem_node.label.clone(),
                source_file: path.clone(),
                source_line: None,
                docstring: Some(sem_node.summary.clone()),
                signature: None,
                node_type: sem_node.node_type.clone(),
            });
        }
        for sem_edge in &sem_ext.edges {
            ext.edges.push(astria_extract::ExtractedEdge {
                source: sem_edge.source.clone(),
                target: sem_edge.target.clone(),
                relation: sem_edge.relation.clone(),
                confidence: "SEMANTIC".to_string(),
                confidence_score: sem_edge.confidence_score,
                source_file: path.clone(),
                source_line: None,
            });
        }
    }
    Ok(SemanticPassStats {
        enriched: pending_paths.len(),
        cached,
        gated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lookup must key on the plain normalized path and accept the
    /// extraction-layer hash family (plain or `:gws-rev:`-suffixed). The
    /// previous version queried a `semantic:`-prefixed path with the
    /// semantic configuration hash and could never match a stored row.
    #[test]
    fn derived_text_lookup_matches_extraction_hash_family() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("doc.pdf");
        std::fs::write(&file, b"%PDF-1.4 fake").unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let plain = astria_extract::cache::file_hash(&file).unwrap();
        let key = astria_paths::normalize(&file);

        // Fresh plain-hash row matches.
        db.execute(
            "INSERT INTO derived_text (file_path, content_hash, text) VALUES (?1, ?2, ?3)",
            rusqlite::params![key, plain, "# extracted"],
        )
        .unwrap();
        assert_eq!(
            load_derived_text(&db, &file).as_deref(),
            Some("# extracted")
        );

        // GWS-suffixed hash (remote revision fingerprint) matches too.
        db.execute(
            "INSERT OR REPLACE INTO derived_text (file_path, content_hash, text) VALUES (?1, ?2, ?3)",
            rusqlite::params![key, format!("{plain}:gws-rev:ABC123"), "# gws"],
        )
        .unwrap();
        assert_eq!(load_derived_text(&db, &file).as_deref(), Some("# gws"));

        // Stale hash (file changed since extraction) counts as missing.
        db.execute(
            "INSERT OR REPLACE INTO derived_text (file_path, content_hash, text) VALUES (?1, ?2, ?3)",
            rusqlite::params![key, "0".repeat(64), "# stale"],
        )
        .unwrap();
        assert_eq!(load_derived_text(&db, &file), None);
    }
}
