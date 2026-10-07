//! Pipeline stages: embedding, community labeling, deep-link extraction.
use super::*;

/// A model swap orphans the old vectors: `has_embeddings` is
/// current-model-scoped, so a graph embedded by an older model counts as
/// "no embeddings". Foreign-model rows therefore also trigger the silent
/// refresh — `embed_missing_nodes` re-embeds exactly those rows — so an
/// ordinary `update` heals a model swap. When the current model is not
/// cached the heal cannot run offline; the mismatch is then reported
/// loudly instead of silently disabling semantic recall.
#[cfg(feature = "embed")]
pub(crate) fn embed_stage(db: &Connection, requested: bool) -> astria_core::Result<()> {
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
pub(crate) fn embed_stage(_db: &Connection, requested: bool) -> astria_core::Result<()> {
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
pub(crate) fn label_communities_stage(
    db: &Connection,
    requested: bool,
) -> astria_core::Result<Option<CommunityLabelStats>> {
    label_communities_stage_with(db, requested, astria_semantic::backend_from_env)
}

pub(crate) fn label_communities_stage_with(
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
pub(crate) fn deep_link_stage(
    db: &Connection,
    root: &Path,
    requested: bool,
) -> astria_core::Result<Option<DeepLinkStats>> {
    deep_link_stage_with(db, root, requested, astria_semantic::backend_from_env)
}

pub(crate) fn deep_link_stage_with(
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
