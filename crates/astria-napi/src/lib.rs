pub mod benchmark;
pub mod diagnose;
pub mod export_cypher;
pub mod export_graphml;
pub mod export_html;
pub mod export_obsidian;
pub mod export_tree;
pub mod export_wiki;
// ---------------------------------------------------------------------------
// Diagnose, feedback, global graph, and extra ingest sources
// ---------------------------------------------------------------------------

#[napi(object)]
pub struct DiagnoseReportJs {
    pub node_count: i64,
    pub edge_count: i64,
    pub dangling_edges: i64,
    pub self_loops: i64,
    pub duplicate_edges: i64,
    pub stub_nodes: i64,
    pub unlinked_nodes: i64,
    pub file_type_counts: Vec<String>,
    pub top_dangling_targets: Vec<String>,
    pub text: String,
}

#[napi]
pub fn diagnose_graph(root: String) -> napi::Result<DiagnoseReportJs> {
    let root_pb = PathBuf::from(&root);
    let (db, _) = pipeline::load_graph_db_flexible(&root_pb)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let report = diagnose::diagnose(&db).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(DiagnoseReportJs {
        node_count: report.node_count as i64,
        edge_count: report.edge_count as i64,
        dangling_edges: report.dangling_edges as i64,
        self_loops: report.self_loops as i64,
        duplicate_edges: report.duplicate_edges as i64,
        stub_nodes: report.stub_nodes as i64,
        unlinked_nodes: report.unlinked_nodes as i64,
        file_type_counts: report
            .file_type_counts
            .iter()
            .map(|(ft, c)| format!("{ft}:{c}"))
            .collect(),
        top_dangling_targets: report
            .top_dangling_targets
            .iter()
            .map(|(t, c)| format!("{t}:{c}"))
            .collect(),
        text: diagnose::render(&report),
    })
}

#[napi(object)]
pub struct RiskReportJs {
    pub score: i64,
    pub level: String,
    pub changed_files: Vec<String>,
    pub files_with_symbols: i64,
    pub impacted: i64,
    pub by_depth: Vec<String>,
    pub communities: Vec<String>,
    pub entries: Vec<String>,
    pub text: String,
}

#[napi]
pub fn risk_report(root: String, staged: Option<bool>) -> napi::Result<RiskReportJs> {
    let root_pb = PathBuf::from(&root);
    let root_canon = root_pb
        .canonicalize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let (db, _) = pipeline::load_graph_db_flexible(&root_pb)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let changed = risk::git_changed_files(&root_canon, staged.unwrap_or(false))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let outcome =
        risk::compute_risk(&db, &changed).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let project_root = Some(
        root_canon
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/"),
    );
    let entries = outcome
        .entries
        .iter()
        .map(|e| {
            format!(
                "{} (depth {}) via {} from {}",
                e.label, e.depth, e.relation, e.via_file
            )
        })
        .collect();
    let by_depth = outcome
        .by_depth
        .iter()
        .map(|(d, c)| format!("depth {d}: {c}"))
        .collect();
    let text = risk::render(&outcome, project_root.as_deref());
    Ok(RiskReportJs {
        score: outcome.score as i64,
        level: risk::level_of(outcome.score).to_string(),
        changed_files: outcome.changed_files.clone(),
        files_with_symbols: outcome.files_with_symbols as i64,
        impacted: outcome.impacted as i64,
        by_depth,
        communities: outcome.communities.clone(),
        entries,
        text,
    })
}

#[napi(object)]
pub struct SvgCountsJs {
    pub nodes: i64,
    pub edges: i64,
    pub communities: i64,
    pub truncated: bool,
}

#[napi]
pub fn export_svg_cmd(root: String, out_path: String) -> napi::Result<SvgCountsJs> {
    let root_pb = PathBuf::from(&root);
    let (db, _) = pipeline::load_graph_db_flexible(&root_pb)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let counts = export_svg::export_svg(&db, &PathBuf::from(&out_path))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(SvgCountsJs {
        nodes: counts.nodes as i64,
        edges: counts.edges as i64,
        communities: counts.communities as i64,
        truncated: counts.truncated,
    })
}

#[napi(object)]
pub struct Neo4jPushCountsJs {
    pub nodes: i64,
    pub edges: i64,
    pub communities: i64,
    pub statements: i64,
}

#[napi(js_name = "neo4jPushCmd")]
pub fn neo4j_push_cmd(
    root: String,
    url: String,
    user: Option<String>,
    pass: Option<String>,
) -> napi::Result<Neo4jPushCountsJs> {
    let root_pb = PathBuf::from(&root);
    let (db, _) = pipeline::load_graph_db_flexible(&root_pb)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let user = user
        .or_else(|| std::env::var("NEO4J_USERNAME").ok())
        .unwrap_or_else(|| "neo4j".into());
    let pass = pass
        .or_else(|| std::env::var("NEO4J_PASSWORD").ok())
        .unwrap_or_default();
    let counts = neo4j_push::neo4j_push(&db, &url, &user, &pass)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(Neo4jPushCountsJs {
        nodes: counts.nodes as i64,
        edges: counts.edges as i64,
        communities: counts.communities as i64,
        statements: counts.statements as i64,
    })
}

fn health_grade(score: i64) -> String {
    match score {
        85..=100 => "good".into(),
        70..=84 => "fair".into(),
        _ => "needs attention".into(),
    }
}

#[napi(object)]
pub struct HealthReportJs {
    pub score: i64,
    pub grade: String,
    /// Formatted "label (file) — N outgoing, 0 incoming" lines.
    pub dead_code: Vec<String>,
    /// Formatted "N files: a -> b -> a" lines.
    pub cycles: Vec<String>,
    /// Formatted "label (degree N, community X)" lines.
    pub hubs: Vec<String>,
    pub age_days: Option<i64>,
    pub node_count: i64,
    pub edge_count: i64,
    pub text: String,
}

#[napi]
pub fn health_report(root: String) -> napi::Result<HealthReportJs> {
    let root_pb = PathBuf::from(&root);
    let (db, _) = pipeline::load_graph_db_flexible(&root_pb)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let project_root = std::path::Path::new(&root)
        .canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"));
    let report =
        astria_analyze::health::health(&db).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let dead_code = report
        .dead_code_candidates
        .iter()
        .map(|c| {
            format!(
                "{} ({}) — {} outgoing, 0 incoming",
                c.label, c.source_file, c.outgoing
            )
        })
        .collect();
    let cycles = report
        .cycles
        .iter()
        .map(|c| format!("{} files: {}", c.files.len(), c.files.join(" -> ")))
        .collect();
    let hubs = report
        .hub_churn
        .iter()
        .map(|h| {
            format!(
                "{} (degree {}, community {})",
                h.label,
                h.degree,
                h.community.as_deref().unwrap_or("-")
            )
        })
        .collect();
    let score = report.score as i64;
    let grade = health_grade(score);
    let text = astria_analyze::health::render(&report, project_root.as_deref());
    Ok(HealthReportJs {
        score,
        grade,
        dead_code,
        cycles,
        hubs,
        age_days: report.age_days.map(|d| d as i64),
        node_count: report.node_count as i64,
        edge_count: report.edge_count as i64,
        text,
    })
}

#[napi(object)]
pub struct SavedResultJs {
    pub memory_path: String,
    pub node_id: String,
}

#[napi]
pub fn save_query_result(
    root: String,
    question: String,
    answer: String,
    outcome: Option<String>,
    correction: Option<String>,
    source_nodes: Option<Vec<String>>,
) -> napi::Result<SavedResultJs> {
    let root_pb = PathBuf::from(&root);
    let (db, astria_dir) = pipeline::load_graph_db_flexible(&root_pb)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let dir = astria_dir.ok_or_else(|| {
        napi::Error::from_reason("save-result needs a repo graph (not a bare db file)".to_string())
    })?;
    let saved = feedback::save_result(
        &db,
        &dir,
        &question,
        &answer,
        outcome.as_deref(),
        correction.as_deref(),
        source_nodes.as_deref().unwrap_or(&[]),
    )
    .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(SavedResultJs {
        memory_path: astria_paths::normalize(&saved.memory_path),
        node_id: saved.node_id,
    })
}

#[napi]
pub fn reflect(root: String) -> napi::Result<String> {
    let root_pb = PathBuf::from(&root);
    let (_, astria_dir) = pipeline::load_graph_db_flexible(&root_pb)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let dir = astria_dir.ok_or_else(|| {
        napi::Error::from_reason("reflect needs a repo graph (not a bare db file)".to_string())
    })?;
    feedback::reflect(&dir).map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[napi(object)]
pub struct GlobalAddResultJs {
    pub tag: String,
    pub nodes_added: i64,
    pub edges_added: i64,
    pub same_type_edges: i64,
    pub cross_repo_call_edges: i64,
}

#[napi]
pub fn global_add(root: String, tag: Option<String>) -> napi::Result<GlobalAddResultJs> {
    let repo_root = PathBuf::from(&root);
    let store = global::open_global_store().map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let result = global::global_add(&repo_root, tag.as_deref(), &store)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(GlobalAddResultJs {
        tag: result.tag,
        nodes_added: result.nodes_added as i64,
        edges_added: result.edges_added as i64,
        same_type_edges: result.same_type_edges as i64,
        cross_repo_call_edges: result.cross_repo_call_edges as i64,
    })
}

#[napi]
pub fn global_remove(tag: String) -> napi::Result<i64> {
    let store = global::open_global_store().map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let removed =
        global::global_remove(&store, &tag).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(removed as i64)
}

#[napi(object)]
pub struct GlobalListEntryJs {
    pub tag: String,
    pub nodes: i64,
    pub edges: i64,
}

#[napi]
pub fn global_list() -> napi::Result<Vec<GlobalListEntryJs>> {
    let store = global::open_global_store().map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(global::global_list(&store)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?
        .into_iter()
        .map(|e| GlobalListEntryJs {
            tag: e.tag,
            nodes: e.nodes as i64,
            edges: e.edges as i64,
        })
        .collect())
}

#[napi]
pub fn global_path(source: String, target: String) -> napi::Result<Option<String>> {
    let store = global::open_global_store().map_err(|e| napi::Error::from_reason(e.to_string()))?;
    global::global_path(&store, &source, &target)
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}

/// Ingest a simplified SCIP JSON index into the repo graph.
#[napi(object)]
pub struct IngestCountsJs {
    pub nodes_added: i64,
    pub edges_added: i64,
}

#[napi]
pub fn ingest_scip(root: String, scip_path: String) -> napi::Result<IngestCountsJs> {
    let root_pb = PathBuf::from(&root);
    let astria_dir = root_pb.join(".astria");
    if !astria_dir.exists() {
        return Err(napi::Error::from_reason(format!(
            "No graph found at {} — run `astria run <path>` first",
            astria_dir.display()
        )));
    }
    let db = astria_core::db::open_db(&astria_dir.join("db.sqlite"))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let extraction = astria_ingest::scip::parse_scip_file(&PathBuf::from(&scip_path))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let result = astria_build::build(std::slice::from_ref(&extraction), &db)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(IngestCountsJs {
        nodes_added: result.nodes_added as i64,
        edges_added: result.edges_added as i64,
    })
}

/// Introspect a live PostgreSQL schema via the `psql` CLI and merge it into
/// the repo graph. Read-only; requires psql on PATH.
#[napi]
pub fn ingest_postgres(root: String, dsn: String) -> napi::Result<IngestCountsJs> {
    let root_pb = PathBuf::from(&root);
    let astria_dir = root_pb.join(".astria");
    if !astria_dir.exists() {
        return Err(napi::Error::from_reason(format!(
            "No graph found at {} — run `astria run <path>` first",
            astria_dir.display()
        )));
    }
    let db = astria_core::db::open_db(&astria_dir.join("db.sqlite"))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let extraction = astria_ingest::postgres::ingest_postgres(&dsn)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let result = astria_build::build(std::slice::from_ref(&extraction), &db)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(IngestCountsJs {
        nodes_added: result.nodes_added as i64,
        edges_added: result.edges_added as i64,
    })
}

mod export_svg;
pub mod feedback;
pub mod global;
pub mod merge;
mod neo4j_push;
pub mod pipeline;
pub mod query;
mod risk;

use napi_derive::napi;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---- napi-exposed types ----

#[napi(object)]
pub struct PipelineResultJs {
    pub nodes_added: i64,
    pub edges_added: i64,
    pub communities: i64,
    pub report: String,
    /// Files whose semantic extraction came from the content-hash cache.
    pub semantic_cached: i64,
    /// Files the backend's gate dropped before extraction (not failures).
    pub semantic_gated: i64,
    /// Measured LLM spend of this run's semantic passes.
    pub llm_input_tokens: i64,
    pub llm_output_tokens: i64,
    pub llm_api_calls: i64,
    /// Communities (re)named by the LLM (`--label-communities`), or -1 when
    /// the stage did not run.
    pub communities_labeled: i64,
    pub communities_reused: i64,
    /// Naming calls that failed this run (those communities keep hub names).
    pub communities_failed: i64,
    /// INFERRED concept edges written by `--deep`, or -1 when it did not run.
    pub deep_links: i64,
}

#[napi(object)]
pub struct GraphStatsJs {
    pub node_count: i64,
    pub edge_count: i64,
    pub community_count: i64,
    pub file_count: i64,
    pub type_counts: HashMap<String, i64>,
}

/// Build provenance read from the graph's `_meta` table, so callers can
/// judge freshness without filesystem timestamps.
#[napi(object)]
pub struct GraphBuildInfoJs {
    /// Finished-at timestamp of the most recent completed pipeline run.
    pub graph_published_at: Option<String>,
    /// npm CLI version of the driver that built the graph; None means an
    /// internal caller ran the pipeline without one.
    pub astria_version: Option<String>,
    /// Rust pipeline version that built the graph.
    pub pipeline_version: Option<String>,
    /// Extraction rules version the graph was produced by; None means the
    /// graph predates version stamping.
    pub extraction_hash_version: Option<String>,
    /// Semantic configuration signature of the last build.
    pub build_configuration: Option<String>,
    /// Extraction rules version compiled into this binary; a graph whose
    /// `extraction_hash_version` differs predates current rules.
    pub current_extraction_hash_version: String,
}

/// One hub node from `astria god-nodes` (MCP `god_nodes` parity).
#[napi(object)]
pub struct GodNodeJs {
    pub id: String,
    pub label: String,
    pub degree: i64,
    /// Community label when named, else the numeric id; None when the node
    /// has no community.
    pub community: Option<String>,
}

/// One community from `astria communities` (MCP `list_communities` parity).
#[napi(object)]
pub struct CommunityJs {
    pub id: i64,
    pub label: String,
    pub summary: Option<String>,
    /// `llm` (thematic label) or `hub` (label derived from hub nodes).
    pub label_source: String,
    pub cohesion: Option<f64>,
    pub size: i64,
}

#[napi(object)]
pub struct CommunitiesJs {
    pub modularity: Option<f64>,
    pub communities: Vec<CommunityJs>,
}

#[napi(object)]
pub struct QueryResultJs {
    pub text: String,
    pub node_count: i64,
    pub edge_count: i64,
    /// Some when the node list was truncated - pass back as `cursor`.
    pub next_cursor: Option<i64>,
    /// Finished-at timestamp of the most recent completed pipeline run,
    /// lets callers judge how stale the graph is.
    pub graph_built_at: Option<String>,
}

#[napi(object)]
pub struct RepoMapJs {
    pub text: String,
    pub files_shown: i64,
}

#[napi(object)]
pub struct PathResultJs {
    pub found: bool,
    pub hops: i64,
    pub text: String,
}

#[napi(object)]
pub struct EdgeInfoJs {
    pub neighbor_id: String,
    pub neighbor_label: String,
    pub neighbor_file: String,
    pub neighbor_line: Option<i64>,
    pub relation: String,
    pub confidence: String,
    pub confidence_score: Option<f64>,
}

#[napi(object)]
pub struct ExplainResultJs {
    pub id: String,
    pub label: String,
    pub source_file: String,
    pub source_line: Option<i64>,
    pub community: Option<i64>,
    pub hyperedges: Vec<String>,
    pub neighbor_count: i64,
    pub neighbors: Vec<EdgeInfoJs>,
}

#[napi(object)]
pub struct DiffResultJs {
    pub nodes_added: i64,
    pub nodes_removed: i64,
    pub edges_added: i64,
    pub edges_removed: i64,
    pub added_node_labels: Vec<String>,
    pub removed_node_labels: Vec<String>,
}

#[napi(object)]
pub struct HistoryEntryJs {
    pub id: i64,
    pub question: String,
    pub answer: Option<String>,
    pub queried_at: String,
}

#[napi(object)]
pub struct AffectedHitJs {
    pub id: String,
    pub label: String,
    pub depth: i32,
    pub relation: String,
    /// `EXTRACTED` (edge present in the source) or `INFERRED` (reconstructed
    /// from name references — direction not guaranteed).
    pub provenance: String,
    pub via_file: String,
}

#[napi(object)]
pub struct AffectedResultJs {
    pub seed: String,
    pub seed_label: String,
    pub total: i32,
    pub hits: Vec<AffectedHitJs>,
}

// ---- napi-exposed functions ----

/// Fidelity tier: "high" keeps only EXTRACTED/DECLARED facts (strength
/// >= 0.9); anything else keeps all facts.
fn min_strength_for(detail: &Option<String>) -> f64 {
    match detail.as_deref().map(|s| s.to_lowercase()).as_deref() {
        Some("high") => 0.9,
        _ => 0.0,
    }
}

#[napi]
pub fn run_pipeline(
    root: String,
    no_dedup: Option<bool>,
    embed: Option<bool>,
    label_communities: Option<bool>,
    deep: Option<bool>,
    cli_version: Option<String>,
) -> napi::Result<PipelineResultJs> {
    let root_pb = PathBuf::from(&root);
    let result = pipeline::run_pipeline_with(
        &root_pb,
        !no_dedup.unwrap_or(false),
        embed.unwrap_or(false),
        label_communities.unwrap_or(false),
        deep.unwrap_or(false),
        cli_version.as_deref(),
    )
    .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(pipeline_result_js(&result))
}

/// Incremental rebuild — intentionally reuses run_pipeline because the pipeline
/// internally detects changed files via SHA-256 manifest and skips unchanged ones.
#[napi]
pub fn update_pipeline(
    root: String,
    no_dedup: Option<bool>,
    embed: Option<bool>,
    label_communities: Option<bool>,
    deep: Option<bool>,
    cli_version: Option<String>,
) -> napi::Result<PipelineResultJs> {
    let root_pb = PathBuf::from(&root);
    let result = pipeline::run_pipeline_with(
        &root_pb,
        !no_dedup.unwrap_or(false),
        embed.unwrap_or(false),
        label_communities.unwrap_or(false),
        deep.unwrap_or(false),
        cli_version.as_deref(),
    )
    .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(pipeline_result_js(&result))
}

/// Flatten a pipeline result for JS. Stats that did not run report -1 so
/// "ran and produced zero" stays distinguishable from "not requested".
fn pipeline_result_js(result: &pipeline::PipelineResult) -> PipelineResultJs {
    PipelineResultJs {
        nodes_added: result.build_result.nodes_added as i64,
        edges_added: result.build_result.edges_added as i64,
        communities: result.cluster_result.communities.len() as i64,
        report: result.report.clone(),
        semantic_cached: result.semantic_cached as i64,
        semantic_gated: result.semantic_gated as i64,
        llm_input_tokens: result.llm_usage.input as i64,
        llm_output_tokens: result.llm_usage.output as i64,
        llm_api_calls: result.llm_usage.calls as i64,
        communities_labeled: result
            .community_labels
            .map(|s| s.labeled as i64)
            .unwrap_or(-1),
        communities_reused: result
            .community_labels
            .map(|s| s.reused as i64)
            .unwrap_or(-1),
        communities_failed: result
            .community_labels
            .map(|s| s.failed as i64)
            .unwrap_or(-1),
        deep_links: result
            .deep_links
            .map(|s| s.links_added as i64)
            .unwrap_or(-1),
    }
}

#[napi]
pub fn graph_stats(root: String) -> napi::Result<GraphStatsJs> {
    let db = pipeline::load_graph_db(&PathBuf::from(&root))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let node_count: i64 = db
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .unwrap_or(0);
    let edge_count: i64 = db
        .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
        .unwrap_or(0);
    let community_count: i64 = db
        .query_row(
            "SELECT COUNT(DISTINCT community) FROM nodes WHERE community IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let file_count: i64 = db
        .query_row("SELECT COUNT(*) FROM file_manifest", [], |r| r.get(0))
        .unwrap_or(0);
    let mut stmt = db
        .prepare("SELECT file_type, COUNT(*) FROM nodes GROUP BY file_type ORDER BY COUNT(*) DESC")
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let type_counts: HashMap<String, i64> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(GraphStatsJs {
        node_count,
        edge_count,
        community_count,
        file_count,
        type_counts,
    })
}

/// Read one `_meta` value from a graph database, None when the key (or the
/// table) is absent.
fn meta_value(db: &rusqlite::Connection, key: &str) -> Option<String> {
    db.query_row("SELECT value FROM _meta WHERE key = ?1", [key], |r| {
        r.get(0)
    })
    .ok()
}

/// Community id -> hub label, so god-node output names communities instead
/// of printing raw numbers (mirrors the astria-mcp helper).
fn community_label_map(db: &rusqlite::Connection) -> HashMap<i64, String> {
    let mut stmt = match db.prepare("SELECT id, label FROM communities") {
        Ok(s) => s,
        Err(_) => return Default::default(),
    };
    stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

#[napi]
pub fn graph_build_info(root: String) -> napi::Result<GraphBuildInfoJs> {
    let db = pipeline::load_graph_db(&PathBuf::from(&root))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(GraphBuildInfoJs {
        graph_published_at: meta_value(&db, "graph_published_at"),
        astria_version: meta_value(&db, "astria_version"),
        pipeline_version: meta_value(&db, "pipeline_version"),
        extraction_hash_version: meta_value(&db, "extraction_hash_version"),
        build_configuration: meta_value(&db, "build_configuration"),
        current_extraction_hash_version: astria_core::EXTRACTION_HASH_VERSION.to_string(),
    })
}

#[napi]
pub fn god_nodes(root: String) -> napi::Result<Vec<GodNodeJs>> {
    let db = pipeline::load_graph_db(&PathBuf::from(&root))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let analysis =
        astria_analyze::analyze(&db).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let labels = community_label_map(&db);
    Ok(analysis
        .god_nodes
        .into_iter()
        .map(|n| GodNodeJs {
            id: n.id,
            label: n.label,
            degree: n.degree as i64,
            community: n.community.map(|c| {
                labels
                    .get(&(c as i64))
                    .cloned()
                    .unwrap_or_else(|| c.to_string())
            }),
        })
        .collect())
}

#[napi]
pub fn list_communities(root: String) -> napi::Result<CommunitiesJs> {
    let db = pipeline::load_graph_db(&PathBuf::from(&root))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let mut stmt = db
        .prepare(
            "SELECT id, label, summary, label_source, cohesion, size
             FROM communities ORDER BY size DESC",
        )
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(i64, String, Option<String>, String, Option<f64>, i64)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .map_err(|e| napi::Error::from_reason(e.to_string()))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let modularity = db
        .query_row(
            "SELECT CAST(value AS REAL) FROM _meta WHERE key = 'last_modularity'",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(CommunitiesJs {
        modularity,
        communities: rows
            .into_iter()
            .map(
                |(id, label, summary, label_source, cohesion, size)| CommunityJs {
                    id,
                    label,
                    summary,
                    label_source,
                    cohesion,
                    size,
                },
            )
            .collect(),
    })
}

#[napi]
pub fn export_json_cmd(root: String, out_path: String) -> napi::Result<()> {
    let db = pipeline::load_graph_db(&PathBuf::from(&root))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    pipeline::export_json(&db, &PathBuf::from(&out_path))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(())
}

#[napi]
pub fn export_html_cmd(root: String, out_path: String, mode: Option<String>) -> napi::Result<()> {
    let db = pipeline::load_graph_db(&PathBuf::from(&root))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let mode = export_html::HtmlExportMode::parse(mode.as_deref().unwrap_or("standard"))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    export_html::export_html_with_mode(&db, &PathBuf::from(&out_path), mode)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(())
}

#[napi]
pub fn export_graphml_cmd(root: String, out_path: String) -> napi::Result<()> {
    let db = pipeline::load_graph_db(&PathBuf::from(&root))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    export_graphml::export_graphml(&db, &PathBuf::from(&out_path))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(())
}

#[napi]
pub fn export_cypher_cmd(root: String, out_path: String) -> napi::Result<i32> {
    let db = pipeline::load_graph_db(&PathBuf::from(&root))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let count = export_cypher::export_cypher(&db, &PathBuf::from(&out_path))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(count as i32)
}

/// Token-reduction benchmark block for the CLI to print after a pipeline
/// run. Empty string when no sample question matches the graph.
#[napi]
pub fn token_benchmark(root: String) -> napi::Result<String> {
    let root_pb = PathBuf::from(&root)
        .canonicalize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    benchmark::benchmark_for_root(&root_pb).map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[napi]
pub fn callflow_mermaid(
    root: String,
    node: String,
    depth: i64,
    direction: String,
) -> napi::Result<String> {
    let root_pb = PathBuf::from(&root);
    let root_pb = root_pb
        .canonicalize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let (db, astria_dir) = pipeline::load_graph_db_flexible(&root_pb)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let db_path_str = astria_dir
        .as_ref()
        .map(|d| astria_paths::normalize(&d.join("db.sqlite")))
        .unwrap_or_else(|| root.clone());
    query::callflow_mermaid(&db, &db_path_str, &node, depth.max(1) as usize, &direction)
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[napi]
#[allow(clippy::too_many_arguments)]
pub fn query_graph(
    root: String,
    question: String,
    mode: String,
    depth: i64,
    budget: i64,
    directed: Option<bool>,
    detail: Option<String>,
    cursor: Option<i64>,
) -> napi::Result<QueryResultJs> {
    let root_pb = PathBuf::from(&root);
    let root_pb = root_pb
        .canonicalize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    // Load before touching path helpers: a missing graph must error without
    // creating an empty `.astria/` directory as a side effect.
    let (db, astria_dir) = pipeline::load_graph_db_flexible(&root_pb)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let db_path_str = astria_dir
        .as_ref()
        .map(|d| astria_paths::normalize(&d.join("db.sqlite")))
        .unwrap_or_else(|| root.clone());
    let started = std::time::Instant::now();

    // `--detail high` also prefers file-level nodes when rendering answers.
    let prefer_files = min_strength_for(&detail) >= 0.9;
    let ((text, node_count, edge_count, next_cursor), graph_built_at) =
        query::query_graph_with_metadata(
            &db,
            &db_path_str,
            &question,
            &mode,
            depth as usize,
            budget,
            directed.unwrap_or(false),
            min_strength_for(&detail),
            cursor.unwrap_or(0).max(0) as usize,
            prefer_files,
        )
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    pipeline::record_query_feedback(
        astria_dir.as_deref(),
        "query",
        &question,
        node_count,
        started.elapsed().as_millis(),
    );
    Ok(QueryResultJs {
        text,
        node_count: node_count as i64,
        edge_count: edge_count as i64,
        next_cursor: next_cursor.map(|c| c as i64),
        graph_built_at,
    })
}

#[napi]
pub fn repo_map(root: String, budget: i64, detail: Option<String>) -> napi::Result<RepoMapJs> {
    let root_pb = PathBuf::from(&root);
    let root_pb = root_pb
        .canonicalize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    // Load before touching path helpers: a missing graph must error without
    // creating an empty `.astria/` directory as a side effect.
    let db =
        pipeline::load_graph_db(&root_pb).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let db_path_str = astria_paths::normalize(&root_pb.join(".astria").join("db.sqlite"));
    let (text, files_shown) = query::repo_map(&db, &db_path_str, budget, min_strength_for(&detail))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(RepoMapJs {
        text,
        files_shown: files_shown as i64,
    })
}

#[napi]
pub fn find_path(
    root: String,
    source: String,
    target: String,
    directed: Option<bool>,
    detail: Option<String>,
) -> napi::Result<PathResultJs> {
    let root_pb = PathBuf::from(&root);
    let root_pb = root_pb
        .canonicalize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    // Load before touching path helpers: a missing graph must error without
    // creating an empty `.astria/` directory as a side effect.
    let (db, astria_dir) = pipeline::load_graph_db_flexible(&root_pb)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let db_path_str = astria_dir
        .as_ref()
        .map(|d| astria_paths::normalize(&d.join("db.sqlite")))
        .unwrap_or_else(|| root.clone());
    let started = std::time::Instant::now();
    let (found, hops, text) = query::find_shortest_path(
        &db,
        &db_path_str,
        &source,
        &target,
        directed.unwrap_or(false),
        min_strength_for(&detail),
    )
    .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    pipeline::record_query_feedback(
        astria_dir.as_deref(),
        "path",
        &format!("{source} -> {target}"),
        hops,
        started.elapsed().as_millis(),
    );
    Ok(PathResultJs {
        found,
        hops: hops as i64,
        text,
    })
}

#[napi]
pub fn explain_node(root: String, node_id: String) -> napi::Result<Option<ExplainResultJs>> {
    let root_pb = PathBuf::from(&root);
    let root_pb = root_pb
        .canonicalize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    // Load before touching path helpers: a missing graph must error without
    // creating an empty `.astria/` directory as a side effect.
    let (db, astria_dir) = pipeline::load_graph_db_flexible(&root_pb)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let db_path_str = astria_dir
        .as_ref()
        .map(|d| astria_paths::normalize(&d.join("db.sqlite")))
        .unwrap_or_else(|| root.clone());
    let started = std::time::Instant::now();
    let result = query::explain_with_neighbors(&db, &db_path_str, &node_id)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    if let Some(r) = &result {
        pipeline::record_query_feedback(
            astria_dir.as_deref(),
            "explain",
            &node_id,
            r.neighbor_count,
            started.elapsed().as_millis(),
        );
    }
    Ok(result.map(|r| ExplainResultJs {
        id: r.id,
        label: r.label,
        source_file: r.source_file,
        source_line: r.source_line,
        community: r.community,
        hyperedges: r.hyperedges,
        neighbor_count: r.neighbor_count as i64,
        neighbors: r
            .neighbors
            .into_iter()
            .map(|n| EdgeInfoJs {
                neighbor_id: n.neighbor_id,
                neighbor_label: n.neighbor_label,
                neighbor_file: n.neighbor_file,
                neighbor_line: n.neighbor_line,
                relation: n.relation,
                confidence: n.confidence,
                confidence_score: n.confidence_score,
            })
            .collect(),
    }))
}

#[napi]
pub fn affected_node(
    root: String,
    node: String,
    depth: Option<u32>,
    relation: Option<String>,
) -> napi::Result<AffectedResultJs> {
    let root_pb = PathBuf::from(&root);
    // Report via_file hit paths relative to the project root.
    let root_str = root_pb
        .canonicalize()
        .map(|p| astria_paths::normalize(&p))
        .unwrap_or(root.clone());
    let db =
        pipeline::load_graph_db(&root_pb).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let result =
        astria_analyze::affected::affected(&db, &node, depth.unwrap_or(2), relation.as_deref())
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(AffectedResultJs {
        seed: result.seed,
        seed_label: result.seed_label,
        total: result.total as i32,
        hits: result
            .hits
            .into_iter()
            .map(|h| AffectedHitJs {
                id: h.id,
                label: h.label,
                depth: h.depth as i32,
                relation: h.relation,
                provenance: h.provenance,
                via_file: astria_paths::relative_display(&h.via_file, &root_str),
            })
            .collect(),
    })
}

#[napi]
pub fn export_tree(root: String, out: String, max_children: Option<i32>) -> napi::Result<i32> {
    let root_pb = PathBuf::from(&root);
    let db =
        pipeline::load_graph_db(&root_pb).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let out_path = Path::new(&out);
    let out_path = if out_path.is_absolute() {
        out_path.to_path_buf()
    } else {
        root_pb.join(out_path)
    };
    let count =
        export_tree::export_tree(&db, &out_path, max_children.unwrap_or(40).max(1) as usize)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(count as i32)
}

#[napi]
pub fn export_wiki(root: String, out_dir: String, max_key_nodes: Option<i32>) -> napi::Result<i32> {
    let root_pb = PathBuf::from(&root);
    let db =
        pipeline::load_graph_db(&root_pb).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    // Canonicalized so stored absolute source paths strip to root-relative.
    let root_pb = root_pb
        .canonicalize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let out_path = Path::new(&out_dir);
    let out_path = if out_path.is_absolute() {
        out_path.to_path_buf()
    } else {
        root_pb.join(out_path)
    };
    let count = export_wiki::export_wiki(
        &db,
        &out_path,
        max_key_nodes.unwrap_or(25).max(1) as usize,
        Some(&root_pb),
    )
    .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(count as i32)
}

#[napi]
pub fn export_obsidian(root: String, out_dir: String) -> napi::Result<i32> {
    let root_pb = PathBuf::from(&root);
    let db =
        pipeline::load_graph_db(&root_pb).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let out_path = Path::new(&out_dir);
    let out_path = if out_path.is_absolute() {
        out_path.to_path_buf()
    } else {
        root_pb.join(out_path)
    };
    let count = export_obsidian::export_obsidian(&db, &out_path)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(count as i32)
}

/// Debug/introspection: semantic candidates for a question against stored
/// node embeddings. Empty when embeddings or the cached model are absent.
#[napi(object)]
pub struct SemanticCandidateJs {
    pub node_id: String,
    pub cosine: f64,
}

#[napi]
pub fn semantic_candidates(
    root: String,
    question: String,
) -> napi::Result<Vec<SemanticCandidateJs>> {
    #[cfg(feature = "embed")]
    {
        let root_pb = PathBuf::from(&root);
        let db = pipeline::load_graph_db(&root_pb)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        if !astria_embed::has_embeddings(&db) || !astria_embed::model_cached() {
            return Ok(Vec::new());
        }
        let mut embedder =
            astria_embed::load_embedder().map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let scores = astria_embed::semantic_scores(&db, &mut embedder, &question)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(scores
            .into_iter()
            .map(|(node_id, cosine)| SemanticCandidateJs { node_id, cosine })
            .collect())
    }
    #[cfg(not(feature = "embed"))]
    {
        let _ = (root, question);
        Ok(Vec::new())
    }
}

#[napi(object)]
pub struct IngestResultJs {
    pub saved_path: String,
    pub graph_updated: bool,
}

#[napi]
pub fn ingest_url(
    root: String,
    url: String,
    author: Option<String>,
    contributor: Option<String>,
) -> napi::Result<IngestResultJs> {
    let root_pb = PathBuf::from(&root);
    if !root_pb.exists() {
        return Err(napi::Error::from_reason(format!(
            "path does not exist: {}",
            root_pb.display()
        )));
    }

    let opts = astria_ingest::IngestOptions {
        author,
        contributor,
    };
    let raw_dir = root_pb.join("raw");
    let saved = astria_ingest::ingest_url(&url, &raw_dir, &opts)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;

    // Incremental update picks the new file up (hash manifest sees it as new)
    pipeline::run_pipeline_with(&root_pb, true, false, false, false, None)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;

    Ok(IngestResultJs {
        saved_path: astria_paths::normalize(&saved),
        graph_updated: true,
    })
}

/// File-system-safe transcript name: alphanumerics, `-`, `_`, and the
/// extension dot; everything else collapses to `-`. Preserves the original
/// name so re-adding the same source overwrites idempotently instead of
/// stacking timestamped copies.
fn transcript_file_name(source_name: &str) -> String {
    let sanitized: String = source_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Edge dashes and dots go too: a leading dot would hide the file on
    // unix, and trailing separators survive as noise.
    let sanitized = sanitized
        .trim_matches(|c: char| matches!(c, '-' | '.' | '_'))
        .to_string();
    let has_ext = sanitized.rsplit('.').next().is_some_and(|ext| {
        !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric())
    });
    if has_ext {
        sanitized
    } else {
        format!("{sanitized}.md")
    }
}

/// Save a transcript into `.astria/transcripts/` and update the graph —
/// the writer side of the transcript-sidecar contract (any external
/// transcriber can also drop files there directly; this is the built-in
/// path, e.g. `astria add --transcript -` piping from a tool).
///
/// Exactly one of `source` (a file path; the basename is kept) or
/// `content` (raw text; stored under a timestamped name) is required.
#[napi]
pub fn save_transcript(
    root: String,
    source: Option<String>,
    content: Option<String>,
) -> napi::Result<IngestResultJs> {
    let root_pb = PathBuf::from(&root);
    if !root_pb.exists() {
        return Err(napi::Error::from_reason(format!(
            "path does not exist: {}",
            root_pb.display()
        )));
    }
    let (name, text) = match (source, content) {
        (Some(source), None) => {
            let path = PathBuf::from(&source);
            let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
                napi::Error::from_reason(format!("transcript path has no file name: {source}"))
            })?;
            let text = std::fs::read_to_string(&path).map_err(|e| {
                napi::Error::from_reason(format!("cannot read transcript {source}: {e}"))
            })?;
            (transcript_file_name(name), text)
        }
        (None, Some(content)) => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            (format!("transcript-{ts}.md"), content)
        }
        (Some(_), Some(_)) => {
            return Err(napi::Error::from_reason(
                "pass either a transcript source file or inline content, not both",
            ))
        }
        (None, None) => {
            return Err(napi::Error::from_reason(
                "nothing to save: pass a transcript source file or inline content",
            ))
        }
    };
    if text.trim().is_empty() {
        return Err(napi::Error::from_reason(
            "transcript is empty; refusing to save an empty sidecar",
        ));
    }
    let dir = root_pb.join(".astria").join("transcripts");
    std::fs::create_dir_all(&dir)
        .map_err(|e| napi::Error::from_reason(format!("cannot create {}: {e}", dir.display())))?;
    let saved = dir.join(&name);
    std::fs::write(&saved, &text)
        .map_err(|e| napi::Error::from_reason(format!("cannot write {}: {e}", saved.display())))?;

    // Incremental update picks the sidecar up as a document (the detect
    // walk covers .astria/transcripts explicitly).
    pipeline::run_pipeline_with(&root_pb, true, false, false, false, None)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;

    Ok(IngestResultJs {
        saved_path: astria_paths::normalize(&saved),
        graph_updated: true,
    })
}

#[napi]
pub fn run_mcp_server(root: String) -> napi::Result<()> {
    let root_pb = PathBuf::from(&root);
    if !root_pb.exists() {
        return Err(napi::Error::from_reason(format!(
            "path does not exist: {}",
            root_pb.display()
        )));
    }
    let root_pb = root_pb
        .canonicalize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    // Refuse to serve (or create) a graph that was never built: the MCP
    // server is read-only, so a missing graph is a hard error — otherwise
    // agents would connect to an empty graph with no hint why.
    let db_path = root_pb.join(".astria").join("db.sqlite");
    if !db_path.exists() {
        return Err(napi::Error::from_reason(format!(
            "No graph found at {} — run `astria run <path>` first",
            db_path.display()
        )));
    }
    astria_mcp::serve(&db_path).map_err(|e| napi::Error::from_reason(e.to_string()))
}

#[napi]
pub fn cluster_only(root: String) -> napi::Result<PipelineResultJs> {
    let root_pb = PathBuf::from(&root);
    let root_pb = root_pb
        .canonicalize()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let db =
        pipeline::load_graph_db(&root_pb).map_err(|e| napi::Error::from_reason(e.to_string()))?;

    let cluster_result =
        astria_cluster::cluster(&db).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let analysis =
        astria_analyze::analyze(&db).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let report = astria_report::generate_report(&db, &analysis)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;

    let astria_dir = root_pb.join(".astria");
    let _ = std::fs::write(astria_dir.join("graph_report.md"), &report);

    Ok(PipelineResultJs {
        nodes_added: 0,
        edges_added: 0,
        communities: cluster_result.communities.len() as i64,
        report,
        semantic_cached: 0,
        semantic_gated: 0,
        llm_input_tokens: 0,
        llm_output_tokens: 0,
        llm_api_calls: 0,
        communities_labeled: -1,
        communities_reused: -1,
        communities_failed: -1,
        deep_links: -1,
    })
}

#[napi]
pub fn merge_graphs(
    root_a: String,
    root_b: String,
    out_root: String,
) -> napi::Result<PipelineResultJs> {
    let result = merge::merge_graphs(
        &PathBuf::from(&root_a),
        &PathBuf::from(&root_b),
        &PathBuf::from(&out_root),
    )
    .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(PipelineResultJs {
        nodes_added: result.nodes_added,
        edges_added: result.edges_added,
        communities: result.communities as i64,
        report: result.report,
        semantic_cached: 0,
        semantic_gated: 0,
        llm_input_tokens: 0,
        llm_output_tokens: 0,
        llm_api_calls: 0,
        communities_labeled: -1,
        communities_reused: -1,
        communities_failed: -1,
        deep_links: -1,
    })
}

#[napi]
pub fn diff_graphs(root_a: String, root_b: String) -> napi::Result<DiffResultJs> {
    let result = merge::diff_graphs(&PathBuf::from(&root_a), &PathBuf::from(&root_b))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(DiffResultJs {
        nodes_added: result.nodes_added,
        nodes_removed: result.nodes_removed,
        edges_added: result.edges_added,
        edges_removed: result.edges_removed,
        added_node_labels: result.added_node_labels,
        removed_node_labels: result.removed_node_labels,
    })
}

#[napi]
pub fn graph_history(root: String, limit: i64) -> napi::Result<Vec<HistoryEntryJs>> {
    let db = pipeline::load_graph_db(&PathBuf::from(&root))
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let mut stmt = db
        .prepare(
            "SELECT id, question, answer, queried_at FROM query_history ORDER BY id DESC LIMIT ?1",
        )
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let entries: Vec<HistoryEntryJs> = stmt
        .query_map(rusqlite::params![limit], |row| {
            Ok(HistoryEntryJs {
                id: row.get(0)?,
                question: row.get(1)?,
                answer: row.get(2)?,
                queried_at: row.get(3)?,
            })
        })
        .map_err(|e| napi::Error::from_reason(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use crate::pipeline;
    use crate::query;
    use crate::transcript_file_name;
    use astria_core::db::open_db_in_memory;

    fn seed_graph(
        db: &rusqlite::Connection,
        nodes: &[(&str, &str, &str, Option<i64>)],
        edges: &[(&str, &str, &str)],
    ) {
        for &(id, label, sf, community) in nodes {
            db.execute(
                "INSERT INTO nodes (id, label, file_type, source_file, community) VALUES (?1, ?2, 'code', ?3, ?4)",
                rusqlite::params![id, label, sf, community],
            ).unwrap();
        }
        for &(src, tgt, rel) in edges {
            db.execute(
                "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES (?1, ?2, ?3, 'EXTRACTED', 'test.py')",
                rusqlite::params![src, tgt, rel],
            ).unwrap();
        }
    }

    #[test]
    fn query_graph_empty_db_returns_no_nodes() {
        let db = open_db_in_memory().unwrap();
        let key = format!(":memory:empty_{}", std::process::id());
        let (text, nodes, edges, _) =
            query::query_graph(&db, &key, "anything", "bfs", 3, 2000, false, 0.0, 0, false)
                .unwrap();
        assert_eq!(text, "No nodes in graph.\n");
        assert_eq!(nodes, 0);
        assert_eq!(edges, 0);
    }

    #[test]
    fn query_graph_no_matching_nodes() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db, &[("n1", "Alpha", "f.py", None)], &[]);
        let key = format!(":memory:nomatch_{}", std::process::id());
        let (text, nodes, _, _) = query::query_graph(
            &db,
            &key,
            "xyznonexistent",
            "bfs",
            3,
            2000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        assert_eq!(text, "No matching nodes found.\n");
        assert_eq!(nodes, 0);
    }

    #[test]
    fn query_graph_bfs_finds_subgraph() {
        let db = open_db_in_memory().unwrap();
        seed_graph(
            &db,
            &[
                ("n1", "Alpha", "f.py", Some(0)),
                ("n2", "Beta", "f.py", Some(0)),
                ("n3", "Gamma", "g.py", Some(1)),
            ],
            &[("n1", "n2", "calls"), ("n2", "n3", "imports")],
        );
        let key = format!(":memory:bfs_{}", std::process::id());
        let (text, nodes, _edges, _) =
            query::query_graph(&db, &key, "Alpha", "bfs", 2, 2000, false, 0.0, 0, false).unwrap();
        assert!(nodes > 0);
        assert!(text.contains("Alpha"));
    }

    #[test]
    fn query_graph_dfs_finds_subgraph() {
        let db = open_db_in_memory().unwrap();
        seed_graph(
            &db,
            &[("n1", "Alpha", "f.py", None), ("n2", "Beta", "f.py", None)],
            &[("n1", "n2", "calls")],
        );
        let key = format!(":memory:dfs_{}", std::process::id());
        let (text, nodes, _, _) =
            query::query_graph(&db, &key, "Alpha", "dfs", 2, 2000, false, 0.0, 0, false).unwrap();
        assert!(nodes > 0);
        assert!(text.contains("Alpha"));
    }

    #[test]
    fn find_shortest_path_found() {
        let db = open_db_in_memory().unwrap();
        seed_graph(
            &db,
            &[
                ("n1", "Alpha", "f.py", None),
                ("n2", "Beta", "f.py", None),
                ("n3", "Gamma", "g.py", None),
            ],
            &[("n1", "n2", "calls"), ("n2", "n3", "calls")],
        );
        let key = format!(":memory:path_{}", std::process::id());
        let (found, hops, text) =
            query::find_shortest_path(&db, &key, "Alpha", "Gamma", false, 0.0).unwrap();
        assert!(found);
        assert_eq!(hops, 2);
        assert!(text.contains("Alpha"));
        assert!(text.contains("Gamma"));
    }

    #[test]
    fn find_shortest_path_exact_id_wins_over_fuzzy() {
        // Reproduces the path defect: exact qualified ids were fed to fuzzy
        // scoring, so "fetch_bytes" top-ranked an unrelated "as_bytes" node
        // and the returned path connected the wrong endpoints entirely.
        let db = open_db_in_memory().unwrap();
        seed_graph(
            &db,
            &[
                ("src_lib::ingest_url", "ingest_url()", "ing.rs", None),
                ("src_lib::fetch_bytes", "fetch_bytes()", "ing.rs", None),
                ("decoy_as_bytes", "as_bytes", "cli.ts", None),
            ],
            &[("src_lib::ingest_url", "src_lib::fetch_bytes", "calls")],
        );
        let key = format!(":memory:path_exact_{}", std::process::id());
        let (found, hops, text) = query::find_shortest_path(
            &db,
            &key,
            "src_lib::ingest_url",
            "src_lib::fetch_bytes",
            false,
            0.0,
        )
        .unwrap();
        assert!(found);
        assert_eq!(hops, 1);
        assert!(
            text.contains("ingest_url"),
            "source endpoint missing: {text}"
        );
        assert!(
            text.contains("fetch_bytes"),
            "target endpoint missing: {text}"
        );
        assert!(!text.contains("as_bytes"), "decoy leaked into path: {text}");
    }

    #[test]
    fn find_shortest_path_no_path() {
        let db = open_db_in_memory().unwrap();
        seed_graph(
            &db,
            &[("n1", "Alpha", "f.py", None), ("n2", "Beta", "f.py", None)],
            &[],
        );
        let key = format!(":memory:nopath_{}", std::process::id());
        let (found, hops, _) =
            query::find_shortest_path(&db, &key, "Alpha", "Beta", false, 0.0).unwrap();
        assert!(!found);
        assert_eq!(hops, 0);
    }

    #[test]
    fn find_shortest_path_no_match() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db, &[("n1", "Alpha", "f.py", None)], &[]);
        let key = format!(":memory:nomatchpath_{}", std::process::id());
        let (found, _, text) =
            query::find_shortest_path(&db, &key, "Alpha", "Nonexistent", false, 0.0).unwrap();
        assert!(!found);
        assert!(text.contains("No matching node"));
    }

    #[test]
    fn find_shortest_path_same_node() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db, &[("n1", "Alpha", "f.py", None)], &[]);
        let key = format!(":memory:same_{}", std::process::id());
        let (found, hops, _) =
            query::find_shortest_path(&db, &key, "Alpha", "Alpha", false, 0.0).unwrap();
        assert!(found);
        assert_eq!(hops, 0);
    }

    #[test]
    fn explain_node_found() {
        let db = open_db_in_memory().unwrap();
        seed_graph(
            &db,
            &[
                ("n1", "Alpha", "f.py", Some(0)),
                ("n2", "Beta", "f.py", Some(0)),
            ],
            &[("n1", "n2", "calls")],
        );
        let key = format!(":memory:explain_{}", std::process::id());
        let result = query::explain_with_neighbors(&db, &key, "n1").unwrap();
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.label, "Alpha");
        assert_eq!(r.neighbor_count, 1);
        assert_eq!(r.neighbors[0].neighbor_label, "Beta");
    }

    #[test]
    fn explain_node_not_found() {
        let db = open_db_in_memory().unwrap();
        let key = format!(":memory:explainnf_{}", std::process::id());
        let result = query::explain_with_neighbors(&db, &key, "nonexistent_xyz").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn export_json_writes_valid_file() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db, &[("n1", "Alpha", "f.py", Some(0))], &[]);
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("graph.json");
        pipeline::export_json(&db, &out).unwrap();
        let json_str = std::fs::read_to_string(&out).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed["nodes"].as_array().unwrap().len() == 1);
        assert!(parsed["edges"].as_array().unwrap().is_empty());
    }

    #[test]
    fn transcript_names_stay_filesystem_safe() {
        // recognized extensions pass through with the original name intact
        assert_eq!(
            transcript_file_name("conv26-session01.md"),
            "conv26-session01.md"
        );
        assert_eq!(transcript_file_name("notes.txt"), "notes.txt");
        // path separators and spaces collapse to dashes; leading traversal
        // segments are trimmed, so nothing escapes the transcripts dir and
        // no dot-prefixed hidden file appears
        assert_eq!(transcript_file_name("../evil name"), "evil-name.md");
        assert_eq!(
            transcript_file_name("..\\evil\\\\name with spaces"),
            "evil--name-with-spaces.md"
        );
        // no recognizable extension gains .md
        assert_eq!(
            transcript_file_name("standup-2026-09-30"),
            "standup-2026-09-30.md"
        );
    }
}
