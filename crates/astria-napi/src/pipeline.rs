use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
/// (enriched, failed) file counts — failures are reported, never silently
/// dropped.
fn enrich_with_semantics(
    files: &[PathBuf],
    extractions: &mut Vec<astria_extract::Extraction>,
    db: &Connection,
) -> (usize, usize) {
    let backend_factory = || astria_semantic::backend_from_env();
    // Gate on backend availability before doing any work.
    if backend_factory().is_err() {
        return (0, 0);
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

        if check_semantic_cache(db, file_path, &hash).is_none() {
            pending.push(Pending {
                path: file_path.clone(),
                hash,
                idx,
            });
        }
    }

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
    (enriched, failed)
}

pub fn run_pipeline(root: &Path) -> astria_core::Result<PipelineResult> {
    run_pipeline_with(root, true, false)
}

/// Run the pipeline with explicit dedup control (`--no-dedup`).
pub fn run_pipeline_with(
    root: &Path,
    dedup: bool,
    embed: bool,
) -> astria_core::Result<PipelineResult> {
    let root = if root.exists() {
        root.canonicalize().map_err(astria_core::AstriaError::Io)?
    } else {
        return Err(astria_core::AstriaError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path does not exist: {}", root.display()),
        )));
    };
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

    let result = run_pipeline_inner(&root, &db, &astria_dir, dedup, embed);

    // Record pipeline completion
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
        "UPDATE pipeline_runs SET finished_at = ?1, status = ?2, files_processed = ?3, nodes_added = ?4, edges_added = ?5 WHERE id = ?6",
        rusqlite::params![timestamp(), status, files_processed, nodes_added, edges_added, run_id],
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

fn run_pipeline_inner(
    root: &Path,
    db: &Connection,
    astria_dir: &Path,
    dedup: bool,
    embed: bool,
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

    if files_to_process.is_empty() && detected.removed.is_empty() && !embed_wanted {
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
        });
    }

    let mut extractions = astria_extract::extract(&files_to_process, db)?;
    let (semantic_enriched, semantic_failed) =
        enrich_with_semantics(&files_to_process, &mut extractions, db);
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
        rusqlite::params![semantic_enriched.to_string()],
    ) {
        eprintln!("warning: failed to record semantic count: {}", e);
    }
    if semantic_failed > 0 {
        let _ = db.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_semantic_failed', ?1)",
            rusqlite::params![semantic_failed.to_string()],
        );
    }

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

    let graph = serde_json::json!({ "nodes": nodes, "edges": edges, "hyperedges": hyperedges });
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
