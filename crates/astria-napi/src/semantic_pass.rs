use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn timestamp() -> String {
    super::timestamp()
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
        });
    }
    backend_factory()?;

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

    // Batch-extract cache misses in parallel.
    let pending_paths: Vec<PathBuf> = pending.iter().map(|p| p.path.clone()).collect();
    let results = astria_semantic::extract_semantic_for_files_parallel(
        &pending_paths,
        backend_factory,
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

    let failed = pending.len() - extraction_by_path.len();
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
                confidence_score: None,
                source_file: path.clone(),
                source_line: None,
            });
        }
    }
    Ok(SemanticPassStats {
        enriched: pending.len(),
        cached,
    })
}
