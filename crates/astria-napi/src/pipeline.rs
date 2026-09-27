use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use astria_core::db;
use astria_paths;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct PipelineResult {
    pub build_result: astria_build::BuildResult,
    pub cluster_result: astria_cluster::ClusterResult,
    pub analysis: astria_analyze::AnalysisResult,
    pub report: String,
    /// Number of new/changed files processed in this run.
    pub files_processed: usize,
    /// Files whose semantic extraction came from the content-hash cache
    /// instead of an API call.
    pub semantic_cached: usize,
    /// Measured LLM spend of this run's semantic passes (extraction,
    /// community naming, deep linking).
    pub llm_usage: astria_semantic::enrichment::TokenUsage,
    /// Thematic community naming stats, when `--label-communities` ran.
    pub community_labels: Option<CommunityLabelStats>,
    /// Deep concept-linking stats, when `--deep` ran.
    pub deep_links: Option<DeepLinkStats>,
}

/// Outcome of the `--label-communities` stage.
#[derive(Debug, Clone, Copy)]
pub struct CommunityLabelStats {
    /// Communities (re)named by the LLM this run.
    pub labeled: usize,
    /// Communities whose LLM label was still valid (membership unchanged).
    pub reused: usize,
    /// Naming calls that failed — those communities keep their hub label.
    pub failed: usize,
}

/// Outcome of the `--deep` stage.
#[derive(Debug, Clone, Copy)]
pub struct DeepLinkStats {
    /// Files whose concept links were (re)computed this run.
    pub files_linked: usize,
    /// INFERRED concept edges written.
    pub links_added: usize,
    /// Files skipped because their cached links were still valid.
    pub files_cached: usize,
}

fn timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn semantic_cache_key(path: &Path) -> String {
    format!("semantic:{}", astria_paths::normalize(path))
}

fn file_hash(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("{:x}", hasher.finalize()))
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
    .map(|(nodes_json, edges_json)| {
        let nodes: Vec<astria_semantic::SemanticNode> =
            serde_json::from_str(&nodes_json).unwrap_or_default();
        let edges: Vec<astria_semantic::SemanticEdge> =
            serde_json::from_str(&edges_json).unwrap_or_default();
        astria_semantic::SemanticExtraction { nodes, edges }
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
/// No-op if no semantic backend is configured (ASTRIA_LLM_API_KEY /
/// OPENAI_API_KEY / GEMINI_API_KEY, or an explicit ASTRIA_LLM_BACKEND).
/// Image files (no AST extraction) get their own synthetic extraction via
/// the backend's vision path. Cache misses are extracted in parallel by a
/// bounded worker pool (`ASTRIA_LLM_CONCURRENCY`, default 4). Returns
/// enriched/failed/cached file counts — failures are reported, never
/// silently dropped.
struct SemanticPassStats {
    enriched: usize,
    failed: usize,
    cached: usize,
}

fn enrich_with_semantics(
    files: &[PathBuf],
    extractions: &mut Vec<astria_extract::Extraction>,
    db: &Connection,
) -> SemanticPassStats {
    let backend_factory = || astria_semantic::backend_from_env();
    // Gate on backend availability before doing any work.
    if backend_factory().is_err() {
        return SemanticPassStats {
            enriched: 0,
            failed: 0,
            cached: 0,
        };
    }

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
    let mut matched = 0usize;
    for file_path in files {
        let hash = match file_hash(file_path) {
            Some(h) => h,
            None => continue,
        };

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
        matched += 1;

        if check_semantic_cache(db, file_path, &hash).is_none() {
            pending.push(Pending {
                path: file_path.clone(),
                hash,
                idx,
            });
        }
    }
    let cached = matched - pending.len();

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

    // Second pass: merge results into the extractions.
    let mut enriched = 0usize;
    let failed = pending.len() - extraction_by_path.len();
    for meta in &pending {
        let Some(sem_ext) = extraction_by_path.get(&meta.path) else {
            continue;
        };
        enriched += 1;
        let ext = &mut extractions[meta.idx];
        for sem_node in &sem_ext.nodes {
            ext.nodes.push(astria_extract::ExtractedNode {
                id: sem_node.id.clone(),
                label: sem_node.label.clone(),
                source_file: meta.path.clone(),
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
                source_file: meta.path.clone(),
                source_line: None,
            });
        }
    }
    SemanticPassStats {
        enriched,
        failed,
        cached,
    }
}

pub fn run_pipeline(root: &Path) -> astria_core::Result<PipelineResult> {
    run_pipeline_with(root, true, false, false, false)
}

/// Run the pipeline with explicit dedup control (`--no-dedup`).
///
/// `label_communities` (`--label-communities`) names communities thematically
/// with one LLM call per changed community; `deep` (`--deep`) adds the
/// cached cross-file concept-linking pass. Both require a semantic backend.
pub fn run_pipeline_with(
    root: &Path,
    dedup: bool,
    embed: bool,
    label_communities: bool,
    deep: bool,
) -> astria_core::Result<PipelineResult> {
    let root = if root.exists() {
        root.canonicalize().map_err(astria_core::AstriaError::Io)?
    } else {
        return Err(astria_core::AstriaError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path does not exist: {}", root.display()),
        )));
    };
    // Fresh accounting for every run: the budget is read once here
    // (ASTRIA_LLM_BUDGET, total tokens, 0 = unlimited) and the counters
    // start at zero so a run's printed spend is its own.
    astria_semantic::enrichment::reset_usage();
    astria_semantic::enrichment::configure_budget(astria_semantic::enrichment::budget_from_env());
    let astria_dir = astria_paths::astria_dir(&root)?;
    let db_path = astria_paths::db_path(&root)?;
    let db = db::open_db(&db_path)?;

    // Stamp the build so stale graphs (e.g. written by an older globally
    // installed binary from a git hook) are detectable in the report.
    let this_version = env!("CARGO_PKG_VERSION");
    let stored_version: Option<String> = db
        .query_row(
            "SELECT value FROM _meta WHERE key = 'pipeline_version'",
            [],
            |r| r.get(0),
        )
        .ok();
    match &stored_version {
        Some(prev) if prev != this_version => {
            eprintln!("[astria] graph was last built by v{prev}, rebuilding with v{this_version}");
        }
        _ => {}
    }
    let _ = db.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('pipeline_version', ?1)",
        rusqlite::params![this_version],
    );

    // Record pipeline start (root is now canonicalized)
    let run_id: i64 = db.query_row(
        "INSERT INTO pipeline_runs (started_at, status) VALUES (?1, 'running') RETURNING id",
        rusqlite::params![timestamp()],
        |row| row.get(0),
    )?;

    let result = run_pipeline_inner(
        &root,
        &db,
        &astria_dir,
        dedup,
        embed,
        label_communities,
        deep,
    );

    // Record pipeline completion, including this run's measured LLM spend.
    let usage = astria_semantic::enrichment::usage_snapshot();
    let (status, files_processed, nodes_added, edges_added) = match &result {
        Ok(r) => (
            "completed",
            r.files_processed as i64,
            r.build_result.nodes_added as i64,
            r.build_result.edges_added as i64,
        ),
        Err(_) => ("failed", 0, 0, 0),
    };
    if let Err(e) = db.execute(
        "UPDATE pipeline_runs SET finished_at = ?1, status = ?2, files_processed = ?3, nodes_added = ?4, edges_added = ?5, llm_input_tokens = ?6, llm_output_tokens = ?7, llm_api_calls = ?8 WHERE id = ?9",
        rusqlite::params![
            timestamp(),
            status,
            files_processed,
            nodes_added,
            edges_added,
            usage.input as i64,
            usage.output as i64,
            usage.calls as i64,
            run_id
        ],
    ) {
        eprintln!("warning: failed to record pipeline status: {}", e);
    }

    result
}

/// Semantic similarity stage: embed nodes missing vectors, then regenerate
/// `similar_to` edges. Runs when explicitly requested, or as a silent
/// incremental refresh when embeddings already exist and the model cache is
/// present (never downloads on its own). Explicit requests fail loudly.
#[cfg(feature = "embed")]
fn embed_stage(db: &Connection, requested: bool) -> astria_core::Result<()> {
    if !requested && !astria_embed::has_embeddings(db) {
        return Ok(());
    }
    // The silent refresh path must stay offline: bail unless cached.
    if !requested && !astria_embed::model_cached() {
        return Ok(());
    }
    match astria_embed::load_embedder() {
        Ok(mut embedder) => {
            let embedded = astria_embed::embed_missing_nodes(db, &mut embedder, 64)?;
            let edges = astria_embed::rebuild_similarity_edges(
                db,
                astria_embed::DEFAULT_SIMILARITY_THRESHOLD,
                astria_embed::DEFAULT_TOP_K,
            )?;
            let _ = db.execute(
                "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_similar_edges', ?1)",
                rusqlite::params![edges.to_string()],
            );
            if requested || embedded > 0 {
                eprintln!("[astria] semantic: {embedded} nodes embedded, {edges} similar_to edges (local model, no API key)");
            }
            Ok(())
        }
        Err(e) => {
            if requested {
                Err(e)
            } else {
                eprintln!("[astria] skipping semantic refresh: {e}");
                Ok(())
            }
        }
    }
}

/// Builds without the `embed` feature (release targets with no prebuilt
/// ONNX Runtime, e.g. x86_64-apple-darwin) reject --embed clearly.
#[cfg(not(feature = "embed"))]
fn embed_stage(_db: &Connection, requested: bool) -> astria_core::Result<()> {
    if requested {
        return Err(astria_core::AstriaError::Graph(
            "semantic embeddings are not supported in this build (no local model runtime for this platform)".to_string(),
        ));
    }
    Ok(())
}

/// `--label-communities`: one LLM call per changed community replaces the
/// hub-symbol label with a thematic name plus a one-line summary. Labels
/// are cached on `member_hash` — unchanged membership is never re-billed —
/// and failures fall back to the hub label, never to a broken community.
fn label_communities_stage(
    db: &Connection,
    requested: bool,
) -> astria_core::Result<Option<CommunityLabelStats>> {
    label_communities_stage_with(db, requested, astria_semantic::backend_from_env)
}

fn label_communities_stage_with(
    db: &Connection,
    requested: bool,
    backend_factory: fn() -> astria_core::Result<Box<dyn astria_semantic::SemanticBackend>>,
) -> astria_core::Result<Option<CommunityLabelStats>> {
    if !requested {
        return Ok(None);
    }
    let backend = backend_factory().map_err(|_| {
        astria_core::AstriaError::Graph(
            "--label-communities needs a semantic backend (set ASTRIA_LLM_API_KEY / OPENAI_API_KEY / GEMINI_API_KEY)"
                .to_string(),
        )
    })?;
    // Call ceiling per run: biggest communities are labeled first, so the
    // cap degrades gracefully instead of blocking the feature entirely.
    let max_calls = astria_core::env_var("LLM_COMMUNITY_MAX")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(48);
    // Communities smaller than this stay hub-named — naming a 2-symbol
    // group spends a call to say what the hub symbol already says.
    const MIN_SIZE: i64 = 3;

    let communities: Vec<(i64, String, i64)> = {
        let mut stmt =
            db.prepare("SELECT id, label, size FROM communities ORDER BY size DESC, id ASC")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let mut stats = CommunityLabelStats {
        labeled: 0,
        reused: 0,
        failed: 0,
    };
    let mut calls = 0usize;
    for (id, hub_label, size) in communities {
        let members: Vec<(String, String)> = {
            let mut stmt = db.prepare(
                "SELECT id, label FROM nodes WHERE community = ?1
                 ORDER BY degree_centrality DESC, id ASC LIMIT 40",
            )?;
            let rows = stmt.query_map(rusqlite::params![id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.filter_map(|r| r.ok()).collect()
        };
        if members.is_empty() {
            continue;
        }

        // Membership fingerprint: shared with cluster(), which stores it on
        // every rebuild — a label is reused only when the source is LLM and
        // the membership is byte-identical to the last labeling.
        let member_hash = {
            let mut ids: Vec<&str> = members.iter().map(|(i, _)| i.as_str()).collect();
            ids.sort_unstable();
            astria_core::db::community_member_hash(&ids)
        };
        let (stored_source, stored_hash): (String, Option<String>) = db
            .query_row(
                "SELECT label_source, member_hash FROM communities WHERE id = ?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or_else(|_| ("hub".to_string(), None));
        if stored_source == "llm" && stored_hash.as_deref() == Some(member_hash.as_str()) {
            stats.reused += 1;
            continue;
        }
        if size < MIN_SIZE {
            continue;
        }
        if calls >= max_calls {
            break;
        }
        if astria_semantic::enrichment::budget_exceeded() {
            break;
        }

        let member_labels: Vec<String> = members.iter().map(|(_, label)| label.clone()).collect();
        calls += 1;
        match astria_semantic::enrichment::summarize_community(
            backend.as_ref(),
            &hub_label,
            size as usize,
            &member_labels,
        ) {
            Ok(naming) => {
                if let Err(e) = db.execute(
                    "UPDATE communities SET label = ?1, summary = ?2, label_source = 'llm', member_hash = ?3 WHERE id = ?4",
                    rusqlite::params![naming.label, naming.summary, member_hash, id],
                ) {
                    eprintln!("warning: failed to store label for community {id}: {e}");
                    stats.failed += 1;
                } else {
                    stats.labeled += 1;
                }
            }
            Err(e) => {
                eprintln!("warning: community naming failed for [{id}] {hub_label}: {e}");
                stats.failed += 1;
            }
        }
        // Gentle pacing between auxiliary calls, mirroring extraction.
        std::thread::sleep(Duration::from_millis(200));
    }

    let _ = db.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_community_labels', ?1)",
        rusqlite::params![format!(
            "labeled={} reused={} failed={}",
            stats.labeled, stats.reused, stats.failed
        )],
    );
    Ok(Some(stats))
}

/// `--deep`: the second extraction tier. For every file, one LLM call
/// links the file's code symbols to concept nodes that live in *other*
/// files — the cross-file concept mesh the AST cannot see. Results are
/// cached per file content hash and written as INFERRED edges tagged
/// `context='deep'`, so rebuilds are idempotent and free when nothing
/// changed.
fn deep_link_stage(
    db: &Connection,
    root: &Path,
    requested: bool,
) -> astria_core::Result<Option<DeepLinkStats>> {
    deep_link_stage_with(db, root, requested, astria_semantic::backend_from_env)
}

fn deep_link_stage_with(
    db: &Connection,
    root: &Path,
    requested: bool,
    backend_factory: fn() -> astria_core::Result<Box<dyn astria_semantic::SemanticBackend>>,
) -> astria_core::Result<Option<DeepLinkStats>> {
    if !requested {
        return Ok(None);
    }
    let backend = backend_factory().map_err(|_| {
        astria_core::AstriaError::Graph(
            "--deep needs a semantic backend (set ASTRIA_LLM_API_KEY / OPENAI_API_KEY / GEMINI_API_KEY)"
                .to_string(),
        )
    })?;

    // The concept menu every file links into: semantic concept nodes
    // (built by extraction), strongest first.
    let concepts: Vec<(String, String)> = {
        let mut stmt = db.prepare(
            "SELECT id, label FROM nodes
             WHERE file_type IN ('concept', 'entity', 'pattern', 'module', 'function')
             ORDER BY degree_centrality DESC, id ASC LIMIT 80",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    let concept_ids: std::collections::HashSet<String> =
        concepts.iter().map(|(id, _)| id.clone()).collect();
    let mut stats = DeepLinkStats {
        files_linked: 0,
        links_added: 0,
        files_cached: 0,
    };
    if concepts.is_empty() {
        eprintln!(
            "[astria] deep: no concept nodes to link against yet — run with a semantic backend first"
        );
        return Ok(Some(stats));
    }

    let files: Vec<(String, String)> = {
        let mut stmt = db.prepare("SELECT file_path, content_hash FROM file_manifest")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    for (manifest_path, hash) in files {
        // file_manifest stores root-relative paths; nodes.source_file is
        // normalized absolute. Join (an already-absolute path joins as-is)
        // so symbol lookup matches what build wrote.
        let file_path = astria_paths::normalize(&root.join(&manifest_path));
        let symbols: Vec<(String, String)> = {
            let mut stmt = db.prepare(
                "SELECT id, label FROM nodes WHERE source_file = ?1 AND file_type = 'code'
                 ORDER BY degree_centrality DESC, id ASC LIMIT 60",
            )?;
            let rows = stmt.query_map(rusqlite::params![&file_path], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.filter_map(|r| r.ok()).collect()
        };
        if symbols.is_empty() {
            continue;
        }
        let cache_key = format!("deep:{file_path}");
        let cached: bool = db
            .query_row(
                "SELECT 1 FROM extraction_cache WHERE file_path = ?1 AND content_hash = ?2",
                rusqlite::params![&cache_key, &hash],
                |r| r.get::<_, i64>(0),
            )
            .is_ok();
        if cached {
            stats.files_cached += 1;
            continue;
        }
        if astria_semantic::enrichment::budget_exceeded() {
            break;
        }

        let display = astria_paths::relative_display(&file_path, &root.to_string_lossy());
        let symbol_labels: Vec<String> = symbols.iter().map(|(_, label)| label.clone()).collect();
        match astria_semantic::enrichment::link_concepts(
            backend.as_ref(),
            &display,
            &symbol_labels,
            &concepts,
        ) {
            Ok(links) => {
                let links_json = serde_json::to_string(&links).unwrap_or_else(|_| "[]".into());
                let tx = db.unchecked_transaction()?;
                // Idempotent re-linking: yesterday's deep edges for this
                // file are replaced wholesale.
                tx.execute(
                    "DELETE FROM edges WHERE source_file = ?1 AND context = 'deep'",
                    rusqlite::params![&file_path],
                )?;
                let label_to_id: HashMap<&str, &str> = symbols
                    .iter()
                    .map(|(id, label)| (label.as_str(), id.as_str()))
                    .collect();
                let mut added = 0usize;
                for link in &links {
                    let (Some(&source_id), true) = (
                        label_to_id.get(link.symbol.as_str()),
                        concept_ids.contains(&link.concept),
                    ) else {
                        continue;
                    };
                    if tx.execute(
                        "INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file, context)
                         VALUES (?1, ?2, ?3, 'INFERRED', 0.5, ?4, 'deep')",
                        rusqlite::params![
                            source_id,
                            link.concept,
                            link.relation,
                            file_path
                        ],
                    )
                    .is_ok()
                    {
                        added += 1;
                    }
                }
                tx.execute(
                    "INSERT OR REPLACE INTO extraction_cache (file_path, content_hash, language, nodes, edges, extracted_at) VALUES (?1, ?2, 'deep', '[]', ?3, ?4)",
                    rusqlite::params![&cache_key, &hash, links_json, timestamp()],
                )?;
                tx.commit()?;
                stats.files_linked += 1;
                stats.links_added += added;
            }
            Err(e) => {
                eprintln!("warning: deep linking failed for {display}: {e}");
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let _ = db.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_deep_links', ?1)",
        rusqlite::params![format!(
            "files={} links={} cached={}",
            stats.files_linked, stats.links_added, stats.files_cached
        )],
    );
    Ok(Some(stats))
}

fn run_pipeline_inner(
    root: &Path,
    db: &Connection,
    astria_dir: &Path,
    dedup: bool,
    embed: bool,
    label_communities: bool,
    deep: bool,
) -> astria_core::Result<PipelineResult> {
    let detected = astria_detect::detect(root, db)?;
    astria_detect::update_manifest(&detected, db)?;

    // Clean up removed files from the graph (transactional)
    if !detected.removed.is_empty() {
        let tx = db.unchecked_transaction()?;
        for entry in &detected.removed {
            // Match how build stores them: absolute path joined from root
            let path_str = astria_paths::normalize(&root.join(&entry.path));
            // Delete edges owned by this file
            tx.execute(
                "DELETE FROM edges WHERE source_file = ?1",
                rusqlite::params![path_str],
            )?;
            // Delete edges from other files that reference nodes being removed
            tx.execute(
                "DELETE FROM edges WHERE source IN (SELECT id FROM nodes WHERE source_file = ?1) OR target IN (SELECT id FROM nodes WHERE source_file = ?1)",
                rusqlite::params![path_str],
            )?;
            tx.execute(
                "DELETE FROM nodes WHERE source_file = ?1",
                rusqlite::params![path_str],
            )?;
            tx.execute(
                "DELETE FROM extraction_cache WHERE file_path = ?1",
                rusqlite::params![path_str],
            )?;
            let semantic_key = format!("semantic:{}", path_str);
            tx.execute(
                "DELETE FROM extraction_cache WHERE file_path = ?1",
                rusqlite::params![semantic_key],
            )?;
            let deep_key = format!("deep:{}", path_str);
            tx.execute(
                "DELETE FROM extraction_cache WHERE file_path = ?1",
                rusqlite::params![deep_key],
            )?;
        }
        tx.commit()?;
    }

    let mut files_to_process: Vec<PathBuf> = detected
        .new
        .iter()
        .chain(detected.changed.iter())
        .map(|e| root.join(&e.path))
        .collect();

    // Transcript sidecars: `.astria/transcripts/*.{txt,md}` become document
    // nodes. This is the local contract of the official astria's whisper
    // step — any transcriber (or hand-written notes) drops a file here and it
    // joins the graph on the next run. The walk skips `.astria/` by design,
    // so these are ingested explicitly.
    let transcripts_dir = astria_dir.join("transcripts");
    if transcripts_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&transcripts_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                let is_sidecar = p
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x == "txt" || x == "md")
                    .unwrap_or(false);
                if is_sidecar && !files_to_process.contains(&p) {
                    files_to_process.push(p);
                }
            }
        }
    }

    // A no-op pass (nothing changed) still runs the embed stage when it is
    // requested or embeddings already exist — otherwise `run --embed` on an
    // unchanged tree would never compute anything.
    #[cfg(feature = "embed")]
    let embed_wanted = embed || astria_embed::has_embeddings(db);
    #[cfg(not(feature = "embed"))]
    let embed_wanted = embed;

    if files_to_process.is_empty()
        && detected.removed.is_empty()
        && !embed_wanted
        && !label_communities
        && !deep
    {
        // Nothing changed on disk, but queries may have accumulated new
        // pairs worth promoting — the report should reflect them.
        let _ = astria_query::promote_learned_edges(db, 2, 3);
        let analysis = astria_analyze::analyze(db)?;
        let report = astria_report::generate_report(db, &analysis)?;
        write_report(astria_dir, &report)?;
        export_json(db, &astria_dir.join("graph.json"))?;
        // Report the communities that exist in the DB, not an empty
        // placeholder - the CLI prints this count on every no-op update.
        let community_count: i64 = db
            .query_row("SELECT COUNT(*) FROM communities", [], |r| r.get(0))
            .unwrap_or(0);
        return Ok(PipelineResult {
            build_result: astria_build::BuildResult {
                nodes_added: 0,
                edges_added: 0,
                duplicates_merged: 0,
            },
            cluster_result: astria_cluster::ClusterResult {
                communities: (0..community_count.max(0) as u32).map(|i| (i, 0)).collect(),
                labels: Default::default(),
                iterations: 0,
                modularity: 0.0,
            },
            analysis,
            report,
            files_processed: 0,
            semantic_cached: 0,
            llm_usage: astria_semantic::enrichment::usage_snapshot(),
            community_labels: None,
            deep_links: None,
        });
    }

    let mut extractions = astria_extract::extract(&files_to_process, root, db)?;
    let semantic_stats = enrich_with_semantics(&files_to_process, &mut extractions, db);
    let build_result = astria_build::build(&extractions, db)?;

    // Entity dedup runs after build, before clustering — duplicate nodes
    // poison community detection and god-node rankings.
    let dedup_merged = if dedup {
        astria_build::dedup::dedup_nodes(db)?
    } else {
        0
    };
    if let Err(e) = db.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_dedup_merged', ?1)",
        rusqlite::params![dedup_merged.to_string()],
    ) {
        eprintln!("warning: failed to record dedup count: {}", e);
    }
    if let Err(e) = db.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_semantic_enriched', ?1)",
        rusqlite::params![semantic_stats.enriched.to_string()],
    ) {
        eprintln!("warning: failed to record semantic count: {}", e);
    }
    if semantic_stats.failed > 0 {
        let _ = db.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_semantic_failed', ?1)",
            rusqlite::params![semantic_stats.failed.to_string()],
        );
    }

    // Deep concept linking runs BEFORE embeddings and clustering so its
    // cross-file INFERRED edges shape both: concept nodes bridge files the
    // AST never connected.
    let deep_stats = deep_link_stage(db, root, deep)?;

    // Semantic similarity pass (local embeddings, no API key): embed new
    // nodes and regenerate similar_to edges BEFORE clustering so they shape
    // communities and analysis. Explicit --embed fails loudly; the silent
    // auto-refresh path never triggers a model download.
    embed_stage(db, embed)?;

    // Feedback loop: promote query pairs that recurred across distinct
    // questions into learned edges. Best-effort — a failure here must not
    // block the build.
    match astria_query::promote_learned_edges(db, 2, 3) {
        Ok(n) if n > 0 => {
            let _ = db.execute(
                "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_learned_edges', ?1)",
                rusqlite::params![n.to_string()],
            );
        }
        Ok(_) => {}
        Err(e) => eprintln!("warning: learned-edge promotion failed: {e}"),
    }

    let cluster_result = astria_cluster::cluster(db)?;

    // Thematic community naming runs after clustering (fresh memberships)
    // and before hyperedges/wiki exports so every downstream surface —
    // report, MCP list_communities, graph.json — sees the good names.
    let label_stats = label_communities_stage(db, label_communities)?;

    // Hyperedges: deterministic N-ary groups (communities, shared references).
    // Best-effort — a failure here must not block the build.
    match astria_build::hyperedges::generate(db) {
        Ok(n) if n > 0 => {
            let _ = db.execute(
                "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_hyperedges', ?1)",
                rusqlite::params![n.to_string()],
            );
        }
        Ok(_) => {}
        Err(e) => eprintln!("warning: hyperedge generation failed: {e}"),
    }
    let analysis = astria_analyze::analyze(db)?;
    let report = astria_report::generate_report(db, &analysis)?;

    write_report(astria_dir, &report)?;
    export_json(db, &astria_dir.join("graph.json"))?;

    Ok(PipelineResult {
        build_result,
        cluster_result,
        analysis,
        report,
        files_processed: files_to_process.len(),
        semantic_cached: semantic_stats.cached,
        llm_usage: astria_semantic::enrichment::usage_snapshot(),
        community_labels: label_stats,
        deep_links: deep_stats,
    })
}

fn write_report(astria_dir: &Path, report: &str) -> astria_core::Result<()> {
    std::fs::write(astria_dir.join("graph_report.md"), report)?;
    Ok(())
}

pub fn export_json(db: &Connection, out_path: &Path) -> astria_core::Result<()> {
    let mut nodes = Vec::new();
    let mut stmt = db.prepare(
        "SELECT id, label, file_type, source_file, source_line, docstring, community, signature FROM nodes",
    )?;
    #[allow(clippy::type_complexity)]
    let node_rows: Vec<(
        String,
        String,
        String,
        String,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
    )> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (id, label, ft, sf, line, doc, comm, sig) in &node_rows {
        nodes.push(serde_json::json!({
            "id": id,
            "label": label,
            "file_type": ft,
            "source_file": sf,
            "source_line": line,
            "docstring": doc,
            "community": comm,
            "signature": sig,
        }));
    }

    let mut edges = Vec::new();
    let mut stmt = db.prepare(
        "SELECT source, target, relation, confidence, confidence_score, source_file FROM edges",
    )?;
    let edge_rows: Vec<(String, String, String, String, Option<f64>, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (src, tgt, rel, conf, score, sf) in &edge_rows {
        edges.push(serde_json::json!({
            "source": src,
            "target": tgt,
            "relation": rel,
            "confidence": conf,
            "confidence_score": score,
            "source_file": sf,
        }));
    }

    // Hyperedges (schema-compatible with the official astria consumer).
    let hyperedges: Vec<serde_json::Value> = match astria_build::hyperedges::load_all(db) {
        Ok(list) => list
            .iter()
            .map(|h| {
                serde_json::json!({
                    "id": h.id,
                    "label": h.label,
                    "nodes": h.nodes,
                    "relation": h.relation,
                    "confidence": h.confidence,
                    "confidence_score": h.score,
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    // Communities with their labels: thematic/hub fallback or LLM-named
    // (label_source='llm'), so agents reading graph.json get group intent,
    // not just group membership ids.
    let communities: Vec<serde_json::Value> = {
        let mut stmt = db.prepare(
            "SELECT id, label, summary, label_source, cohesion, size FROM communities ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        rows.filter_map(|r| r.ok())
            .map(|(id, label, summary, source, cohesion, size)| {
                serde_json::json!({
                    "id": id,
                    "label": label,
                    "summary": summary,
                    "label_source": source,
                    "cohesion": cohesion,
                    "size": size,
                })
            })
            .collect()
    };

    let graph = serde_json::json!({
        "nodes": nodes,
        "edges": edges,
        "hyperedges": hyperedges,
        "communities": communities,
    });
    let json = serde_json::to_string_pretty(&graph)?;
    std::fs::write(out_path, json)?;
    Ok(())
}

/// Open an existing graph database. Unlike `db::open_db`, this never creates
/// anything — a missing graph is a hard error so read-only commands (stats,
/// query, explain, export, ...) fail loudly instead of silently materializing
/// an empty `.astria/` directory wherever they were pointed.
pub fn load_graph_db(root: &Path) -> astria_core::Result<Connection> {
    let p = root.join(".astria").join("db.sqlite");
    if !p.exists() {
        return Err(astria_core::AstriaError::Graph(format!(
            "No graph found at {} — run `astria run <path>` first",
            p.display()
        )));
    }
    db::open_db(&p)
}

/// Open a graph database from either a repo root (`<root>/.astria/db.sqlite`)
/// or a direct path to a `.sqlite` file — lets `query`/`explain`/`path` run
/// against the cross-repo global store via `--graph <path>`. Returns the
/// connection and the db file's parent directory (the `.astria` dir, when
/// known) for stamp/feedback writing.
pub fn load_graph_db_flexible(path: &Path) -> astria_core::Result<(Connection, Option<PathBuf>)> {
    if path.is_file() {
        return Ok((db::open_db(path)?, path.parent().map(|p| p.to_path_buf())));
    }
    let repo_db = path.join(".astria").join("db.sqlite");
    if repo_db.exists() {
        return Ok((
            db::open_db(&repo_db)?,
            Some(repo_db.parent().unwrap().to_path_buf()),
        ));
    }
    Err(astria_core::AstriaError::Graph(format!(
        "No graph found at {} — pass a repo root or a graph .sqlite file",
        path.display()
    )))
}

/// After a successful query/explain/path: append the env-gated JSONL query
/// log and refresh the orientation stamp that `hook-guard read --strict`
/// uses to suppress blocking. Both are best-effort and fail silent.
pub fn record_query_feedback(
    astria_dir: Option<&Path>,
    kind: &str,
    question: &str,
    nodes: usize,
    duration_ms: u128,
) {
    // Orientation stamp (fresh on every query → hook strict TTL).
    if let Some(dir) = astria_dir {
        let cache = dir.join("cache");
        if cache.is_dir() || std::fs::create_dir_all(&cache).is_ok() {
            let stamp = format!(
                "{}\t{}\t{}\t{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                kind,
                nodes,
                duration_ms
            );
            let _ = std::fs::write(cache.join("last_query_stamp"), stamp);
        }
    }

    // JSONL query log (off by default, matching upstream #1797).
    if astria_core::env_var("QUERY_LOG_DISABLE").as_deref() == Some("1") {
        return;
    }
    let path = match astria_core::env_var("QUERY_LOG") {
        Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
        _ => {
            if astria_core::env_var("QUERY_LOG_ENABLE").as_deref() != Some("1") {
                return;
            }
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_else(|_| ".".to_string());
            std::path::PathBuf::from(home)
                .join(".cache")
                .join("astria-queries.log")
        }
    };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!(
        "{{\"ts\":{},\"kind\":{:?},\"question\":{:?},\"nodes\":{},\"duration_ms\":{}}}\n",
        ts, kind, question, nodes, duration_ms
    );
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_semantic::SemanticBackend;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // The stages take `fn()` factories, so the stub reads its canned
    // replies from statics. Each `complete()` call pops the next reply;
    // an empty queue fails the call like a real backend outage would.
    static REPLIES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    // One static stub shared by every test: serialize the tests that
    // consume replies or assert on the shared counters.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct StubBackend;

    impl SemanticBackend for StubBackend {
        fn extract_semantic(
            &self,
            _content: &str,
            _file_type: &str,
        ) -> astria_core::Result<astria_semantic::SemanticExtraction> {
            Ok(astria_semantic::SemanticExtraction::empty())
        }

        fn complete(&self, _system: &str, _user: &str) -> astria_core::Result<String> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            let mut queue = REPLIES.lock().unwrap();
            match queue.is_empty() {
                true => Err(astria_core::AstriaError::Graph(
                    "stub reply queue empty".into(),
                )),
                false => Ok(queue.remove(0)),
            }
        }
    }

    fn stub_factory() -> astria_core::Result<Box<dyn SemanticBackend>> {
        Ok(Box::new(StubBackend))
    }

    fn push_replies(replies: &[&str]) {
        REPLIES
            .lock()
            .unwrap()
            .extend(replies.iter().map(|s| s.to_string()));
    }

    fn calls() -> usize {
        CALLS.load(Ordering::SeqCst)
    }

    fn reset_stubs() {
        REPLIES.lock().unwrap().clear();
        CALLS.store(0, Ordering::SeqCst);
        astria_semantic::enrichment::reset_usage();
        astria_semantic::enrichment::configure_budget(0);
    }

    fn seed_community_graph(db: &Connection) {
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, degree_centrality) VALUES
                ('a', 'parse_config_file', 'code', 'cfg.py', 3.0),
                ('b', 'read_config', 'code', 'cfg.py', 2.0),
                ('c', 'load_settings', 'code', 'cfg.py', 2.0),
                ('d', 'settings_cache', 'code', 'cfg.py', 1.0);
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('a', 'b', 'calls', 'EXTRACTED', 'cfg.py'),
                ('b', 'c', 'calls', 'EXTRACTED', 'cfg.py'),
                ('c', 'd', 'calls', 'EXTRACTED', 'cfg.py');",
        )
        .unwrap();
        astria_cluster::cluster(db).unwrap();
    }

    #[test]
    fn labeling_replaces_hub_label_and_caches_on_membership() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_stubs();
        let db = astria_core::db::open_db_in_memory().unwrap();
        seed_community_graph(&db);
        push_replies(&[
            r#"{"label": "Configuration Loading", "summary": "Reads and caches settings."}"#,
        ]);

        let stats = label_communities_stage_with(&db, true, stub_factory)
            .unwrap()
            .unwrap();
        assert_eq!(stats.labeled, 1, "the one community got named");
        let (label, summary, source): (String, Option<String>, String) = db
            .query_row(
                "SELECT label, summary, label_source FROM communities LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(label, "Configuration Loading");
        assert_eq!(summary.as_deref(), Some("Reads and caches settings."));
        assert_eq!(source, "llm");

        // Second run, unchanged membership: served from member_hash, no call.
        let calls_before = calls();
        let stats = label_communities_stage_with(&db, true, stub_factory)
            .unwrap()
            .unwrap();
        assert_eq!(stats.reused, 1);
        assert_eq!(stats.labeled, 0);
        assert_eq!(calls(), calls_before, "cache hit must not spend a call");
        reset_stubs();
    }

    #[test]
    fn labeling_skips_tiny_communities() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_stubs();
        let db = astria_core::db::open_db_in_memory().unwrap();
        // Two nodes, one edge: a valid but 2-node community.
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('x', 'small_one', 'code', 's.py'), ('y', 'small_two', 'code', 's.py');
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('x', 'y', 'calls', 'EXTRACTED', 's.py');",
        )
        .unwrap();
        astria_cluster::cluster(&db).unwrap();
        push_replies(&[r#"{"label": "Tiny", "summary": "should never be asked"}"#]);

        let stats = label_communities_stage_with(&db, true, stub_factory)
            .unwrap()
            .unwrap();
        assert_eq!(stats.labeled, 0, "2-node communities keep hub labels");
        assert_eq!(calls(), 0);
        reset_stubs();
    }

    #[test]
    fn labeling_failure_keeps_hub_label() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_stubs();
        let db = astria_core::db::open_db_in_memory().unwrap();
        seed_community_graph(&db);
        // Empty reply queue -> complete() errors for the community.
        let stats = label_communities_stage_with(&db, true, stub_factory)
            .unwrap()
            .unwrap();
        assert_eq!(stats.failed, 1);
        let (label, source): (String, String) = db
            .query_row(
                "SELECT label, label_source FROM communities LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            source, "hub",
            "failed naming must not leave provenance 'llm'"
        );
        assert!(!label.is_empty(), "hub/thematic label survives");
        reset_stubs();
    }

    #[test]
    fn labeling_respects_per_run_call_cap() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_stubs();
        let db = astria_core::db::open_db_in_memory().unwrap();
        // Four disconnected chains -> at least four communities.
        for chain in 0..4 {
            let base = chain * 4;
            let mut sql = String::new();
            for i in 0..4 {
                sql.push_str(&format!(
                    "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('c{base}_{i}', 'config_loader_{base}_{i}', 'code', 'f.py');\n"
                ));
            }
            for i in 0..3 {
                let next = i + 1;
                sql.push_str(&format!(
                    "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('c{base}_{i}', 'c{base}_{next}', 'calls', 'EXTRACTED', 'f.py');\n"
                ));
            }
            db.execute_batch(&sql).unwrap();
        }
        astria_cluster::cluster(&db).unwrap();
        let replies: Vec<String> = (0..16)
            .map(|_| r#"{"label": "Group", "summary": "s"}"#.to_string())
            .collect();
        *REPLIES.lock().unwrap() = replies;
        std::env::set_var("ASTRIA_LLM_COMMUNITY_MAX", "2");
        let stats = label_communities_stage_with(&db, true, stub_factory)
            .unwrap()
            .unwrap();
        std::env::remove_var("ASTRIA_LLM_COMMUNITY_MAX");
        assert_eq!(stats.labeled, 2, "cap stops the run after 2 calls");
        assert_eq!(calls(), 2);
        reset_stubs();
    }

    #[test]
    fn budget_stops_labeling_before_any_call() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_stubs();
        astria_semantic::enrichment::configure_budget(1);
        astria_semantic::enrichment::record_usage(&serde_json::json!(
            {"usage": {"prompt_tokens": 5, "completion_tokens": 5}}
        ));
        let db = astria_core::db::open_db_in_memory().unwrap();
        seed_community_graph(&db);
        let stats = label_communities_stage_with(&db, true, stub_factory)
            .unwrap()
            .unwrap();
        assert_eq!(stats.labeled, 0);
        assert_eq!(calls(), 0, "exhausted budget must spend nothing");
        reset_stubs();
    }

    fn seed_deep_graph(db: &Connection, root: &Path) -> PathBuf {
        let file = root.join("auth.rs");
        std::fs::write(&file, "pub fn login() {}\n").unwrap();
        let path_str = astria_paths::normalize(&file);
        let mut hasher = Sha256::new();
        hasher.update(b"pub fn login() {}\n");
        let hash = format!("{:x}", hasher.finalize());
        db.execute_batch(&format!(
            "INSERT INTO nodes (id, label, file_type, source_file, degree_centrality) VALUES
                ('auth_login', 'login()', 'code', '{path_str}', 2.0),
                ('auth_verify', 'verify_token()', 'code', '{path_str}', 1.0),
                ('concept_auth', 'Authentication', 'concept', 'concepts', 5.0),
                ('concept_jwt', 'JWT tokens', 'concept', 'concepts', 3.0);
             INSERT INTO file_manifest (file_path, content_hash, file_type, last_seen_at, size_bytes)
                VALUES ('{path_str}', '{hash}', 'code', '0', 17);"
        ))
        .unwrap();
        file
    }

    #[test]
    fn deep_linking_writes_inferred_edges_then_caches() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_stubs();
        let dir = tempfile::tempdir().unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        seed_deep_graph(&db, dir.path());
        push_replies(&[r#"{"links": [
            {"symbol": "login()", "concept": "concept_auth", "relation": "implements"},
            {"symbol": "verify_token()", "concept": "concept_jwt", "relation": "uses"},
            {"symbol": "ghost()", "concept": "concept_auth", "relation": "uses"},
            {"symbol": "login()", "concept": "concept_missing", "relation": "uses"}
        ]}"#]);

        let stats = deep_link_stage_with(&db, dir.path(), true, stub_factory)
            .unwrap()
            .unwrap();
        // ghost() is not a node label in the file; concept_missing is not a
        // concept id — both dropped, two real links written.
        assert_eq!(stats.files_linked, 1);
        assert_eq!(stats.links_added, 2);
        let (rel, conf, ctx): (String, String, Option<String>) = db
            .query_row(
                "SELECT relation, confidence, context FROM edges WHERE source = 'auth_login'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((rel.as_str(), conf.as_str()), ("implements", "INFERRED"));
        assert_eq!(ctx.as_deref(), Some("deep"));

        // Re-run on the unchanged file: cache hit, edges untouched, no calls.
        let calls_before = calls();
        let stats = deep_link_stage_with(&db, dir.path(), true, stub_factory)
            .unwrap()
            .unwrap();
        assert_eq!(stats.files_cached, 1);
        assert_eq!(calls(), calls_before);
        let edges: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE context = 'deep'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(edges, 2);
        reset_stubs();
    }

    #[test]
    fn deep_linking_replaces_stale_links_on_hash_change() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_stubs();
        let dir = tempfile::tempdir().unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let file = seed_deep_graph(&db, dir.path());
        push_replies(&[r#"{"links": [
            {"symbol": "login()", "concept": "concept_auth", "relation": "uses"}
        ]}"#]);
        let stats = deep_link_stage_with(&db, dir.path(), true, stub_factory)
            .unwrap()
            .unwrap();
        assert_eq!(stats.links_added, 1);

        // File content changes -> new hash -> fresh (re)link, old edges gone.
        std::fs::write(&file, "pub fn login() {}\npub fn logout() {}\n").unwrap();
        let bytes = std::fs::read(&file).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hash = format!("{:x}", hasher.finalize());
        let path_str = astria_paths::normalize(&file);
        db.execute(
            "UPDATE file_manifest SET content_hash = ?1 WHERE file_path = ?2",
            rusqlite::params![hash, path_str],
        )
        .unwrap();
        db.execute(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('auth_logout', 'logout()', 'code', ?1)",
            rusqlite::params![path_str],
        )
        .unwrap();
        push_replies(&[r#"{"links": [
            {"symbol": "logout()", "concept": "concept_auth", "relation": "uses"}
        ]}"#]);
        let stats = deep_link_stage_with(&db, dir.path(), true, stub_factory)
            .unwrap()
            .unwrap();
        assert_eq!(stats.files_linked, 1);
        let (count, labels): (i64, String) = db
            .query_row(
                "SELECT COUNT(*), COALESCE(GROUP_CONCAT(source), '') FROM edges WHERE context = 'deep'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1, "stale links replaced, not accumulated");
        assert_eq!(labels, "auth_logout");
        reset_stubs();
    }

    #[test]
    fn deep_linking_without_concepts_reports_and_skips() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_stubs();
        let dir = tempfile::tempdir().unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        // Only code nodes: nothing to link against.
        let file = dir.path().join("solo.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let path_str = astria_paths::normalize(&file);
        db.execute_batch(&format!(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('solo_main', 'main()', 'code', '{path_str}');
             INSERT INTO file_manifest (file_path, content_hash, file_type, last_seen_at, size_bytes)
                VALUES ('{path_str}', 'h', 'code', '0', 13);"
        ))
        .unwrap();
        let stats = deep_link_stage_with(&db, dir.path(), true, stub_factory)
            .unwrap()
            .unwrap();
        assert_eq!(stats.files_linked, 0);
        assert_eq!(calls(), 0);
        reset_stubs();
    }

    #[test]
    fn stages_refuse_to_run_without_a_backend() {
        let _guard = TEST_LOCK.lock().unwrap();
        // A factory that always fails -> loud error, not a silent no-op.
        fn broken_factory() -> astria_core::Result<Box<dyn SemanticBackend>> {
            Err(astria_core::AstriaError::Graph("no key".into()))
        }
        let db = astria_core::db::open_db_in_memory().unwrap();
        assert!(label_communities_stage_with(&db, true, broken_factory).is_err());
        let dir = tempfile::tempdir().unwrap();
        assert!(deep_link_stage_with(&db, dir.path(), true, broken_factory).is_err());
        // Not requested -> no error, no work.
        assert!(label_communities_stage_with(&db, false, broken_factory)
            .unwrap()
            .is_none());
        assert!(deep_link_stage_with(&db, dir.path(), false, broken_factory)
            .unwrap()
            .is_none());
    }
}
