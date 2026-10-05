use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use astria_core::db;
use astria_paths;
use rusqlite::Connection;

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
    /// Files the backend's gate dropped before extraction (not failures).
    pub semantic_gated: usize,
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

#[path = "graph_update.rs"]
mod graph_update;
#[path = "semantic_pass.rs"]
mod semantic_pass;

pub fn run_pipeline(root: &Path) -> astria_core::Result<PipelineResult> {
    run_pipeline_with(root, true, false, false, false, None)
}

/// Run the pipeline with explicit dedup control (`--no-dedup`).
///
/// `label_communities` (`--label-communities`) names communities thematically
/// with one LLM call per changed community; `deep` (`--deep`) adds the
/// cached cross-file concept-linking pass. Both require a semantic backend.
/// `cli_version` is the npm package version of the calling driver, stamped
/// into the graph so `astria status` can report which release built it.
pub fn run_pipeline_with(
    root: &Path,
    dedup: bool,
    embed: bool,
    label_communities: bool,
    deep: bool,
    cli_version: Option<&str>,
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
    // The npm CLI version is the one users actually upgrade; warn when the
    // graph came from a different release so staleness is visible here too.
    if let Some(running) = cli_version {
        let stored_cli: Option<String> = db
            .query_row(
                "SELECT value FROM _meta WHERE key = 'astria_version'",
                [],
                |r| r.get(0),
            )
            .ok();
        if let Some(prev) = stored_cli {
            if prev != running {
                eprintln!(
                    "[astria] graph was last built by astria {prev}, this binary is astria {running}"
                );
            }
        }
    }

    // Record pipeline start (root is now canonicalized)
    let run_id: i64 = db.query_row(
        "INSERT INTO pipeline_runs (started_at, status) VALUES (?1, 'running') RETURNING id",
        rusqlite::params![timestamp()],
        |row| row.get(0),
    )?;

    let options = PipelineOptions {
        dedup,
        embed,
        label_communities,
        deep,
        cli_version,
    };
    let result = run_pipeline_inner(&root, &db, &astria_dir, &options);

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

    // Surface the run's spend as `.astria/cost.json` (this run + lifetime
    // totals). Best-effort: a report write must never fail the build.
    if let Err(e) = crate::cost::write_cost_report(&astria_dir, &db, run_id) {
        eprintln!("warning: failed to write cost report: {}", e);
    }

    result
}

/// Semantic similarity stage: embed nodes missing vectors, then regenerate
/// `similar_to` edges. Runs when explicitly requested, or as a silent
/// incremental refresh when embeddings already exist and the model cache is
/// present (never downloads on its own). Explicit requests fail loudly.
///
/// A model swap orphans the old vectors: `has_embeddings` is
/// current-model-scoped, so a graph embedded by an older model counts as
/// "no embeddings". Foreign-model rows therefore also trigger the silent
/// refresh — `embed_missing_nodes` re-embeds exactly those rows — so an
/// ordinary `update` heals a model swap. When the current model is not
/// cached the heal cannot run offline; the mismatch is then reported
/// loudly instead of silently disabling semantic recall.
#[cfg(feature = "embed")]
fn embed_stage(db: &Connection, requested: bool) -> astria_core::Result<()> {
    let foreign: Vec<(String, usize)> = astria_embed::stored_embedding_models(db)?
        .into_iter()
        .filter(|(model, _)| model != astria_embed::MODEL_NAME)
        .collect();
    if !requested && !astria_embed::has_embeddings(db) && foreign.is_empty() {
        return Ok(());
    }
    // The silent refresh path must stay offline: bail unless cached. With
    // foreign-model rows present the bail would strand the graph with
    // vectors no query can use — surface that instead of silence.
    if !requested && !astria_embed::model_cached() {
        if !foreign.is_empty() {
            let stored: Vec<String> = foreign
                .iter()
                .map(|(model, rows)| format!("{model} ({rows} rows)"))
                .collect();
            eprintln!(
                "[astria] stored embeddings use {}, but the current model is {}; semantic query recall is empty until they are re-embedded. Run once with --embed (downloads the model) to migrate.",
                stored.join(", "),
                astria_embed::MODEL_NAME
            );
        }
        return Ok(());
    }
    match astria_embed::load_embedder() {
        Ok(mut embedder) => {
            if !foreign.is_empty() {
                let rows: usize = foreign.iter().map(|(_, n)| n).sum();
                let models: Vec<&str> = foreign.iter().map(|(m, _)| m.as_str()).collect();
                eprintln!(
                    "[astria] embedding model changed: migrating {rows} node vectors from {} to {}",
                    models.join(", "),
                    astria_embed::MODEL_NAME
                );
            }
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
/// are cached by membership, labels, backend configuration, and prompt;
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
            "--label-communities needs an explicit semantic backend (use --backend or ASTRIA_LLM_BACKEND, and configure its credentials)"
                .to_string(),
        )
    })?;
    let backend_configuration = astria_semantic::cache_configuration(backend.as_ref());
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
    for (id, _stored_label, size) in communities {
        let members: Vec<(String, String)> = {
            let mut stmt = db.prepare(
                "SELECT id, label FROM nodes WHERE community = ?1
                 ORDER BY degree_centrality DESC, id ASC",
            )?;
            let rows = stmt.query_map(rusqlite::params![id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.filter_map(|r| r.ok()).collect()
        };
        if members.is_empty() {
            continue;
        }

        // Keep cluster's full membership hash separate from the LLM inputs.
        let hub_label = members[0].1.clone();
        let member_labels: Vec<String> = members.iter().map(|(_, label)| label.clone()).collect();
        let members_json = serde_json::to_string(&members)?;
        let prompt = astria_semantic::enrichment::community_label_user_prompt(
            &hub_label,
            size as usize,
            &member_labels,
        );
        let cache_fingerprint = semantic_pass::fingerprint(&[
            &backend_configuration,
            &members_json,
            astria_semantic::enrichment::community_label_system_prompt(),
            &prompt,
        ]);
        let cache_key = format!("community_label_configuration:{id}");
        let stored_configuration: Option<String> = db
            .query_row(
                "SELECT value FROM _meta WHERE key = ?1",
                [&cache_key],
                |r| r.get(0),
            )
            .ok();
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
        if stored_source == "llm"
            && stored_hash.as_deref() == Some(member_hash.as_str())
            && stored_configuration.as_deref() == Some(cache_fingerprint.as_str())
        {
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

        calls += 1;
        match astria_semantic::enrichment::summarize_community(
            backend.as_ref(),
            &hub_label,
            size as usize,
            &member_labels,
        ) {
            Ok(naming) => {
                let stored = (|| -> astria_core::Result<()> {
                    let tx = db.unchecked_transaction()?;
                    tx.execute(
                        "UPDATE communities SET label = ?1, summary = ?2, label_source = 'llm', member_hash = ?3 WHERE id = ?4",
                        rusqlite::params![naming.label, naming.summary, member_hash, id],
                    )?;
                    tx.execute(
                        "INSERT OR REPLACE INTO _meta (key, value) VALUES (?1, ?2)",
                        rusqlite::params![cache_key, cache_fingerprint],
                    )?;
                    tx.commit()?;
                    Ok(())
                })();
                match stored {
                    Ok(()) => stats.labeled += 1,
                    Err(e) => {
                        eprintln!("warning: failed to store label for community {id}: {e}");
                        stats.failed += 1;
                    }
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
/// cached by content, symbols, concept menu, backend, and prompt; written as INFERRED edges tagged
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
            "--deep needs an explicit semantic backend (use --backend or ASTRIA_LLM_BACKEND, and configure its credentials)"
                .to_string(),
        )
    })?;

    let backend_configuration = astria_semantic::cache_configuration(backend.as_ref());
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
        let display = astria_paths::relative_display(&file_path, &root.to_string_lossy());
        let symbol_labels: Vec<String> = symbols.iter().map(|(_, label)| label.clone()).collect();
        let symbols_json = serde_json::to_string(&symbols)?;
        let concepts_json = serde_json::to_string(&concepts)?;
        let prompt =
            astria_semantic::enrichment::deep_link_user_prompt(&display, &symbol_labels, &concepts);
        let cache_fingerprint = semantic_pass::fingerprint(&[
            &hash,
            &symbols_json,
            &concepts_json,
            &backend_configuration,
            astria_semantic::enrichment::deep_link_system_prompt(),
            &prompt,
        ]);
        let cached: Option<Vec<astria_semantic::enrichment::ConceptLink>> = db
            .query_row(
                "SELECT edges FROM extraction_cache WHERE file_path = ?1 AND content_hash = ?2",
                rusqlite::params![&cache_key, &cache_fingerprint],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok());
        let was_cached = cached.is_some();
        let result = if let Some(links) = cached {
            Ok(links)
        } else {
            if astria_semantic::enrichment::budget_exceeded() {
                break;
            }
            astria_semantic::enrichment::link_concepts(
                backend.as_ref(),
                &display,
                &symbol_labels,
                &concepts,
            )
        };
        // Cached and fresh links follow the same persistence path: rebuilding
        // or deduplicating the base graph can have removed prior deep edges.
        match result {
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
                    rusqlite::params![&cache_key, &cache_fingerprint, links_json, timestamp()],
                )?;
                tx.commit()?;
                if was_cached {
                    stats.files_cached += 1;
                } else {
                    stats.files_linked += 1;
                }
                stats.links_added += added;
            }
            Err(e) => {
                eprintln!("warning: deep linking failed for {display}: {e}");
            }
        }
        if !was_cached {
            std::thread::sleep(Duration::from_millis(200));
        }
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

/// Options threaded from the napi bindings into the pipeline driver; kept
/// in one place so the driver never grows a long positional signature.
#[derive(Default)]
struct PipelineOptions<'a> {
    dedup: bool,
    embed: bool,
    label_communities: bool,
    deep: bool,
    cli_version: Option<&'a str>,
}

fn run_pipeline_inner(
    root: &Path,
    db: &Connection,
    astria_dir: &Path,
    options: &PipelineOptions,
) -> astria_core::Result<PipelineResult> {
    if (options.label_communities || options.deep) && !astria_semantic::enrichment_enabled() {
        return Err(astria_core::AstriaError::Graph(
            "--label-communities and --deep require an explicit --backend or ASTRIA_LLM_BACKEND"
                .into(),
        ));
    }
    // A judge layers decisions on top of an engine; without an engine
    // there is nothing to judge.
    if astria_semantic::judge_from_env().is_some() && !astria_semantic::enrichment_enabled() {
        return Err(astria_core::AstriaError::Graph(
            "ASTRIA_LLM_JUDGE requires an explicit --backend or ASTRIA_LLM_BACKEND \
             (the judge wraps an engine; it cannot generate extractions)"
                .into(),
        ));
    }
    let detected = graph_update::detect(root, db)?;
    let configuration = semantic_pass::configuration()?;
    let build_configuration = format!("{configuration}:dedup={}", options.dedup);
    let previous_configuration: Option<String> = db
        .query_row(
            "SELECT value FROM _meta WHERE key = 'build_configuration'",
            [],
            |row| row.get(0),
        )
        .ok();
    // Files whose external extraction was skipped last run (missing
    // tooling/credentials) are owed a retry: an unchanged tree must still
    // rebuild so installing the dependency is picked up.
    let pending_retry: Vec<String> = db
        .query_row(
            "SELECT value FROM _meta WHERE key = 'pending_retry'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    let files_processed = detected.new.len() + detected.changed.len();
    // F22: Google Workspace shortcuts (.gdoc/.gsheet/.gslides) never change
    // locally when the cloud document is edited, so content-hash detection
    // alone cannot see remote edits. The build decision probes the remote
    // revision for unchanged shortcuts; a moved revision forces a rebuild,
    // and the gws extractor re-fingerprints (and re-exports) on the cache
    // miss. An unreachable revision (offline, no credentials) leaves the
    // cached extraction standing — remote freshness requires connectivity,
    // and offline runs degrade to local hashes by design.
    let gws_stale = stale_workspace_shortcuts(root, db, &detected.unchanged);
    if gws_stale > 0 {
        eprintln!("[astria] {gws_stale} workspace document(s) changed remotely; refreshing");
    }
    let needs_build = files_processed > 0
        || !detected.removed.is_empty()
        || !pending_retry.is_empty()
        || gws_stale > 0
        || previous_configuration.as_deref() != Some(build_configuration.as_str());
    // Resolve against the complete corpus, including cached raw facts for
    // unchanged callers. AST parsing still runs only on cache misses.
    let (build_result, semantic_stats) = if needs_build {
        let files = graph_update::corpus(root, &detected);
        let mut extractions = astria_extract::extract(&files, root, db)?;
        let semantic_stats =
            semantic_pass::enrich_with_semantics(&files, &mut extractions, db, &configuration)?;
        // Deferred extractions (unavailable media tooling, missing workspace
        // credentials) publish nothing this run and stay pending: their old
        // facts are preserved and the next run retries them. Only files
        // whose deferral is ACTIONABLE are deferred: a transcribable media
        // file can succeed once whisper is installed, a workspace shortcut
        // once credentials return. The engine also labels files with NO
        // extractor at all (svg/png/unknown binaries) as language "media" —
        // those can never succeed through the media route, so deferring
        // them would flag the graph dirty on every run forever and force a
        // full republish per no-op update.
        let deferred: Vec<std::path::PathBuf> = extractions
            .iter()
            .filter(|e| {
                if !e.nodes.is_empty() || !e.edges.is_empty() {
                    return false;
                }
                let ext = e
                    .file_path
                    .extension()
                    .and_then(|x| x.to_str())
                    .unwrap_or("");
                e.language == "gws"
                    || (e.language == "media" && astria_core::is_transcribable_extension(ext))
            })
            .map(|e| e.file_path.clone())
            .collect();
        let build_result = graph_update::publish(
            root,
            db,
            &detected,
            &extractions,
            &deferred,
            &build_configuration,
            options.cli_version,
        )?;
        (build_result, semantic_stats)
    } else {
        (
            astria_build::BuildResult {
                nodes_added: 0,
                edges_added: 0,
                duplicates_merged: 0,
            },
            semantic_pass::SemanticPassStats {
                enriched: 0,
                cached: 0,
                gated: 0,
            },
        )
    };

    // Cross-layer linking: docs→packages, packages→entry files, napi FFI
    // imports→Rust functions. Deterministic DB passes (no LLM) that run over
    // the whole graph every pipeline, so incremental updates keep their
    // bridges even when only one side of a link changed. Deterministic from
    // graph content — when nothing upstream changed, the rewrite is
    // content-identical and does not advance the generation.
    match astria_build::crosslayer::link_cross_layer(db) {
        Ok(stats) => {
            let _ = db.execute(
                "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_crosslayer', ?1)",
                rusqlite::params![format!(
                    "ffi_bindings={} entry_points={} doc_refs={}",
                    stats.ffi_bindings, stats.entry_points, stats.doc_refs
                )],
            );
        }
        Err(e) => {
            eprintln!("warning: cross-layer linking failed: {e}");
        }
    };

    // graph_mutated tracks whether THIS run committed a content change; each
    // contributing stage advances the generation immediately (see
    // advance_generation), and the flag decides whether the terminal stamp
    // mints a new generation or reuses the previous one.
    let mut graph_mutated = needs_build;

    // Entity dedup runs after build, before clustering — duplicate nodes
    // poison community detection and god-node rankings.
    let dedup_merged = if options.dedup {
        astria_build::dedup::dedup_nodes(db)?
    } else {
        0
    };
    if dedup_merged > 0 {
        graph_mutated = true;
        advance_generation(db)?;
    }
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
    // Deep concept linking runs BEFORE embeddings and clustering so its
    // cross-file INFERRED edges shape both: concept nodes bridge files the
    // AST never connected.
    let deep_stats = deep_link_stage(db, root, options.deep)?;
    if deep_stats.as_ref().is_some_and(|s| s.links_added > 0) {
        graph_mutated = true;
        advance_generation(db)?;
    }

    // Semantic similarity pass (local embeddings, no API key): embed new
    // nodes and regenerate similar_to edges BEFORE clustering so they shape
    // communities and analysis. Explicit --embed fails loudly; the silent
    // auto-refresh path never triggers a model download.
    let similar_before: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM edges WHERE relation = 'similar_to'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    embed_stage(db, options.embed)?;
    let similar_after: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM edges WHERE relation = 'similar_to'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if similar_after != similar_before {
        graph_mutated = true;
        advance_generation(db)?;
    }

    // Feedback loop: promote query pairs that recurred across distinct
    // questions into learned edges. Best-effort — a failure here must not
    // block the build.
    match astria_query::promote_learned_edges(db, 2, 3) {
        Ok(n) if n > 0 => {
            graph_mutated = true;
            advance_generation(db)?;
            let _ = db.execute(
                "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_learned_edges', ?1)",
                rusqlite::params![n.to_string()],
            );
        }
        Ok(_) => {}
        Err(e) => eprintln!("warning: learned-edge promotion failed: {e}"),
    }

    // Clustering rewrites memberships every run even when the partition is
    // identical; only an assignment that actually CHANGED is a content
    // mutation (and must advance the generation — a later stage can still
    // fail after these writes commit).
    let memberships_before = community_assignments(db);
    let cluster_result = astria_cluster::cluster(db)?;
    if community_assignments(db) != memberships_before {
        graph_mutated = true;
        advance_generation(db)?;
    }

    // Thematic community naming runs after clustering (fresh memberships)
    // and before hyperedges/wiki exports so every downstream surface —
    // report, MCP list_communities, graph.json — sees the good names.
    let label_stats = label_communities_stage(db, options.label_communities)?;
    if let Some(stats) = &label_stats {
        if stats.labeled > 0 || stats.failed > 0 {
            graph_mutated = true;
            advance_generation(db)?;
        }
    }

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
    let mut analysis = astria_analyze::analyze(db)?;
    // Optional derived pass: a decision backend (Jev) may re-rank the
    // suggested questions so the report leads with the most useful ones.
    // Runs only when this run actually built the graph (the questions only
    // change with the graph) and is best-effort — a failure keeps the
    // generated order.
    if needs_build {
        if let Ok(backend) = astria_semantic::backend_from_env() {
            let permutation = backend.rank_questions(&analysis.suggested_questions);
            let questions = &analysis.suggested_questions;
            let mut seen = vec![false; questions.len()];
            let valid_permutation = permutation.len() == questions.len()
                && permutation
                    .iter()
                    .all(|&i| i < questions.len() && !std::mem::replace(&mut seen[i], true));
            if valid_permutation {
                analysis.suggested_questions = permutation
                    .into_iter()
                    .map(|i| questions[i].clone())
                    .collect();
            } else {
                eprintln!(
                    "warning: question ranking returned an invalid permutation; keeping the generated order"
                );
            }
        }
    }
    let report = astria_report::generate_report(db, &analysis)?;

    // One publication workflow: terminal generation stamp (fresh only when
    // the run mutated the graph), report footer, and graph.json all carry
    // the same generation, and the sidecar stamp lands with them.
    publish_artifacts(db, astria_dir, &report, graph_mutated)?;

    Ok(PipelineResult {
        build_result,
        cluster_result,
        analysis,
        report,
        files_processed,
        semantic_cached: semantic_stats.cached,
        semantic_gated: semantic_stats.gated,
        llm_usage: astria_semantic::enrichment::usage_snapshot(),
        community_labels: label_stats,
        deep_links: deep_stats,
    })
}

fn write_report(astria_dir: &Path, report: &str, stamp: &str) -> astria_core::Result<()> {
    // The generation rides in a footer so a consumer can verify the report,
    // graph.json, and database all describe the same publication.
    let body = format!("{report}\n---\n\ngeneration: {stamp}\n");
    write_artifact_atomic(&astria_dir.join("graph_report.md"), body.as_bytes())
}

/// Write a published artifact whole: temp sibling + rename, so a concurrent
/// reader never observes a torn or half-written file.
fn write_artifact_atomic(path: &Path, bytes: &[u8]) -> astria_core::Result<()> {
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
fn stamp_generation(db: &Connection, astria_dir: &Path, mint: bool) -> astria_core::Result<String> {
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
fn stale_workspace_shortcuts(
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

pub fn export_json(db: &Connection, out_path: &Path) -> astria_core::Result<()> {
    // One read snapshot for every table: without a shared transaction a
    // concurrent publisher can land between the node and edge reads and the
    // exported graph would mix two builds.
    let snapshot = db.unchecked_transaction()?;
    export_json_snapshot(&snapshot, out_path)
}

fn export_json_snapshot(
    snapshot: &rusqlite::Transaction<'_>,
    out_path: &Path,
) -> astria_core::Result<()> {
    let db: &Connection = snapshot;
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
        "SELECT source, target, relation, confidence, confidence_score, source_file, source_line, context FROM edges",
    )?;
    #[allow(clippy::type_complexity)]
    let edge_rows: Vec<(
        String,
        String,
        String,
        String,
        Option<f64>,
        String,
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
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // Evidence provenance rides with every edge: the line anchor and the
    // derived-pass ownership ('global'/'deep') are part of the exchange
    // format, matching what SQLite stores.
    for (src, tgt, rel, conf, score, sf, line, context) in &edge_rows {
        edges.push(serde_json::json!({
            "source": src,
            "target": tgt,
            "relation": rel,
            "confidence": conf,
            "confidence_score": score,
            "source_file": sf,
            "source_line": line,
            "context": context,
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
        rows.collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
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

    // Publication metadata: the generation stamped for this build rides with
    // the export so consumers can detect mismatched artifact/database pairs.
    let meta: serde_json::Map<String, serde_json::Value> = {
        let mut stmt = db.prepare("SELECT key, value FROM _meta")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect()
    };

    let graph = serde_json::json!({
        "nodes": nodes,
        "edges": edges,
        "hyperedges": hyperedges,
        "communities": communities,
        "_meta": meta,
    });
    let json = serde_json::to_string_pretty(&graph)?;
    write_artifact_atomic(out_path, json.as_bytes())?;
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
    use sha2::{Digest, Sha256};
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
        assert_eq!(stats.labeled, 0, "2-node communities keep source labels");
        assert_eq!(calls(), 0);
        reset_stubs();
    }

    #[test]
    fn labeling_failure_keeps_source_label() {
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
            source, "source",
            "failed naming must not leave provenance 'llm'"
        );
        assert!(!label.is_empty(), "source module label survives");
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
    fn unextractable_binaries_do_not_keep_the_graph_dirty() {
        // Files with no extractor at all (svg/png/unknown binaries) get an
        // empty language="media" extraction from the engine. They must NOT
        // be counted as deferred media waiting on tooling: nothing can make
        // them succeed through the media route, so deferring them would
        // republish the entire graph on every no-op update forever.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("logo.svg"), b"<svg/>").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "pub fn entry() {}\n").unwrap();

        let first = run_pipeline_with(dir.path(), true, false, false, false, None).unwrap();
        assert!(first.build_result.nodes_added > 0, "first run builds");

        let second = run_pipeline_with(dir.path(), true, false, false, false, None).unwrap();
        assert_eq!(
            second.build_result.nodes_added, 0,
            "no-op update must skip the rebuild: pending_retry must not hold \
             unextractable binaries"
        );
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

    #[test]
    fn advance_generation_replaces_any_previous_stamp() {
        // The failure-path contract: a mid-run stage that committed a content
        // change must leave a generation NO cache can mistake for the
        // previous publication's.
        let db = astria_core::db::open_db_in_memory().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('graph_generation', 'stale')",
            [],
        )
        .unwrap();
        advance_generation(&db).unwrap();
        let stamp: String = db
            .query_row(
                "SELECT value FROM _meta WHERE key = 'graph_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(stamp, "stale");
        assert!(!stamp.is_empty());
    }

    #[test]
    fn terminal_stamp_reuses_generation_on_unchanged_runs() {
        let dir = tempfile::tempdir().unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('graph_generation', 'keep:me')",
            [],
        )
        .unwrap();
        // Unchanged run: the publication identity stays stable so caches and
        // artifacts keep describing the same generation.
        assert_eq!(stamp_generation(&db, dir.path(), false).unwrap(), "keep:me");
        // Mutated run (or a legacy DB with no stamp): mint a fresh one.
        let minted = stamp_generation(&db, dir.path(), true).unwrap();
        assert_ne!(minted, "keep:me");
        assert!(dir.path().join("generation.txt").exists());
    }

    #[test]
    fn community_assignments_reflect_membership_writes() {
        let db = astria_core::db::open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('a', 'A', 'code', 'f.py')",
        )
        .unwrap();
        let before = community_assignments(&db);
        assert_eq!(before.get("a"), Some(&None));
        db.execute("UPDATE nodes SET community = 3 WHERE id = 'a'", [])
            .unwrap();
        let after = community_assignments(&db);
        assert_ne!(before, after, "membership writes must be detectable");
        assert_eq!(after.get("a"), Some(&Some(3)));
    }
}
