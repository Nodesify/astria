//! Graph loading and adjacency: the SQLite snapshot, node/edge records,
//! and traversal helpers over the in-memory petgraph.
//!
//! Split from lib.rs; no behavior change.
#![allow(unused_imports)]

use super::*;
use astria_paths::relative_display;
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

pub(crate) fn log_query(db: &Connection, question: &str, answer: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let _ = db.execute(
        "INSERT INTO query_history (question, answer, path_taken, queried_at) VALUES (?1, ?2, '', ?3)",
        rusqlite::params![question, answer.chars().take(500).collect::<String>(), ts],
    );
}

/// Fallback strength for edges without a numeric score. Alphabetical
/// string comparison of confidence labels does NOT order by strength
/// ("SEMANTIC" > "LLM" lexicographically), so map labels to numbers.
pub(crate) fn confidence_rank(confidence: &str) -> f64 {
    match confidence.to_uppercase().as_str() {
        "DECLARED" => 1.0,
        "EXTRACTED" => 0.9,
        // A call expression extracted from source whose bare name bound to
        // exactly one definition: stronger than co-occurrence inference,
        // deliberately below the EXTRACTED/DECLARED tier so `--detail high`
        // (compiler-grade facts) still excludes it.
        "RESOLVED" => 0.85,
        "INFERRED" => 0.7,
        "SEMANTIC" => 0.6,
        _ => 0.5,
    }
}

pub(crate) struct LoadedGraph {
    /// Directed storage even though most traversals are undirected: the
    /// edge orientation (caller → callee, importer → module) is preserved,
    /// and `directed` queries can follow it.
    pub(crate) graph: DiGraph<NodeData, EdgeData>,
    pub(crate) id_to_idx: HashMap<String, NodeIndex>,
    /// Project root derived from the DB path (`root/.astria/db.sqlite`),
    /// used to shorten stored absolute paths in agent-facing output.
    pub(crate) root: Option<String>,
}

impl LoadedGraph {
    /// Root-relative display form of a stored path.
    pub(crate) fn display_path(&self, path: &str) -> String {
        match &self.root {
            Some(root) => relative_display(path, root),
            None => path.trim_start_matches("//?/").to_string(),
        }
    }
}

/// Process-wide snapshot cache, keyed by database path. Each request used
/// to reload every node and edge (O(V+E) allocations); the HTTP transport
/// multiplies that per request. A cached snapshot is reused only while the
/// publication generation (`_meta.graph_generation`) is unchanged — one
/// cheap scalar query per load replaces the full reload, and read
/// consistency is preserved because each snapshot was loaded atomically
/// and never mutates after construction.
static SNAPSHOT_CACHE: std::sync::Mutex<Option<(String, String, std::sync::Arc<LoadedGraph>)>> =
    std::sync::Mutex::new(None);

/// The database's publication generation, when a writer has stamped one.
/// A MISSING stamp is never treated as a shared empty value: two un-stamped
/// databases (or one database before and after an unstamped write) would
/// compare equal and serve a stale snapshot. Un-stamped databases bypass
/// the cache entirely.
fn generation_of(db: &Connection) -> Option<String> {
    db.query_row(
        "SELECT value FROM _meta WHERE key = 'graph_generation'",
        [],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

pub(crate) fn load_graph(
    db: &Connection,
    db_path: &str,
) -> astria_core::Result<std::sync::Arc<LoadedGraph>> {
    // Cheap freshness probe first: when the generation stamp matches the
    // cached snapshot, share it instead of rebuilding the graph. Databases
    // without a stamp (never written by this pipeline lineage) always load
    // uncached and are never cached.
    let normalized_path = std::path::Path::new(db_path)
        .to_string_lossy()
        .replace('\\', "/");
    let generation = generation_of(db);
    if let Some(generation) = generation.as_ref() {
        let cache = SNAPSHOT_CACHE.lock().unwrap();
        if let Some((cached_path, cached_gen, snapshot)) = cache.as_ref() {
            if *cached_path == normalized_path && cached_gen == generation {
                return Ok(std::sync::Arc::clone(snapshot));
            }
        }
    }

    let loaded = std::sync::Arc::new(load_graph_uncached(db, db_path)?);
    if let Some(generation) = generation {
        *SNAPSHOT_CACHE.lock().unwrap() =
            Some((normalized_path, generation, std::sync::Arc::clone(&loaded)));
    }
    Ok(loaded)
}

fn load_graph_uncached(db: &Connection, db_path: &str) -> astria_core::Result<LoadedGraph> {
    // Project root: two levels above the DB file (root/.astria/db.sqlite).
    let root = std::path::Path::new(db_path)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_string_lossy().replace('\\', "/"));

    let mut nodes = Vec::new();
    {
        let mut stmt = db.prepare(
            "SELECT id, label, file_type, source_file, source_line, community, docstring, signature FROM nodes",
        )?;
        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            String,
            String,
            String,
            String,
            Option<i64>,
            Option<i64>,
            Option<String>,
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
        for (id, label, ft, sf, line, comm, doc, sig) in rows {
            nodes.push((id, label, ft, sf, line, comm, doc, sig));
        }
    }

    let mut graph = DiGraph::new();
    let mut id_to_idx = HashMap::new();
    for (id, label, ft, sf, line, comm, doc, sig) in &nodes {
        let idx = graph.add_node(NodeData {
            id: id.clone(),
            label: label.clone(),
            file_type: ft.clone(),
            source_file: sf.clone(),
            source_line: *line,
            community: *comm,
            docstring: doc.clone(),
            signature: sig.clone(),
        });
        id_to_idx.insert(id.clone(), idx);
    }

    {
        let mut stmt = db.prepare(
            "SELECT source, target, relation, confidence, confidence_score, source_file, source_line FROM edges",
        )?;
        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            String,
            String,
            String,
            String,
            Option<f64>,
            String,
            Option<i64>,
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
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (src, tgt, rel, conf, score, sf, line) in rows {
            if let (Some(&s), Some(&t)) = (id_to_idx.get(&src), id_to_idx.get(&tgt)) {
                graph.add_edge(
                    s,
                    t,
                    EdgeData {
                        relation: rel,
                        confidence: conf,
                        confidence_score: score,
                        source_file: sf,
                        source_line: line,
                    },
                );
            }
        }
    }

    Ok(LoadedGraph {
        graph,
        id_to_idx,
        root,
    })
}

// Read both tables in one SQLite snapshot. Reloading avoids stale state after
// external commits, local writes, and rollbacks.
pub(crate) fn read_snapshot(
    db: &Connection,
) -> astria_core::Result<Option<rusqlite::Transaction<'_>>> {
    Ok(if db.is_autocommit() {
        Some(db.unchecked_transaction()?)
    } else {
        None
    })
}

pub(crate) fn load_graph_snapshot(
    db: &Connection,
    db_path: &str,
) -> astria_core::Result<std::sync::Arc<LoadedGraph>> {
    let transaction = read_snapshot(db)?;
    let loaded = load_graph(db, db_path)?;
    if let Some(transaction) = transaction {
        transaction.commit()?;
    }
    Ok(loaded)
}

/// Neighbors of `idx`: outgoing only when traversing a directed graph,
/// both directions otherwise.
pub(crate) fn iter_neighbors<'a>(
    graph: &'a DiGraph<NodeData, EdgeData>,
    idx: NodeIndex,
    directed: bool,
) -> impl Iterator<Item = NodeIndex> + 'a {
    let outgoing = graph.neighbors_directed(idx, Direction::Outgoing);
    if directed {
        Box::new(outgoing) as Box<dyn Iterator<Item = NodeIndex> + 'a>
    } else {
        Box::new(outgoing.chain(graph.neighbors_directed(idx, Direction::Incoming)))
            as Box<dyn Iterator<Item = NodeIndex> + 'a>
    }
}

/// Like `iter_neighbors`, but only crossing edges whose confidence strength
/// meets `min_strength` — the fidelity-tier filter (`--detail high` keeps
/// only EXTRACTED/DECLARED facts and drops INFERRED/SEMANTIC ones).
pub(crate) fn iter_neighbors_filtered<'a>(
    graph: &'a DiGraph<NodeData, EdgeData>,
    idx: NodeIndex,
    directed: bool,
    min_strength: f64,
    semantic_floor: f64,
) -> impl Iterator<Item = (NodeIndex, EdgeIndex)> + 'a {
    graph
        .edges_directed(idx, Direction::Outgoing)
        .filter(move |e| {
            e.weight().meets_detail(min_strength)
                && !below_semantic_floor(e.weight(), semantic_floor)
        })
        .map(|e| (e.target(), e.id()))
        .chain(
            graph
                .edges_directed(idx, Direction::Incoming)
                .filter(move |e| {
                    !directed
                        && e.weight().meets_detail(min_strength)
                        && !below_semantic_floor(e.weight(), semantic_floor)
                })
                .map(|e| (e.source(), e.id())),
        )
}

/// Opt-in hard floor (`ASTRIA_QUERY_MIN_SEMANTIC_CONFIDENCE`, 0.0 = off):
/// SEMANTIC edges whose calibrated keep-probability falls below the floor
/// are excluded from traversal entirely. Structural and inferred edges are
/// never touched — this is how graph consumers act on the judge's
/// existence verdicts at query time.
pub(crate) fn below_semantic_floor(edge: &EdgeData, floor: f64) -> bool {
    floor > 0.0 && edge.confidence.eq_ignore_ascii_case("SEMANTIC") && edge.strength() < floor
}

/// The strongest edge connecting `a` and `b`, in either direction.
pub(crate) fn edge_between(
    graph: &DiGraph<NodeData, EdgeData>,
    a: NodeIndex,
    b: NodeIndex,
) -> Option<&EdgeData> {
    let forward = graph
        .edges_directed(a, Direction::Outgoing)
        .find(|e| e.target() == b)
        .map(|e| e.weight());
    match forward {
        Some(w) => Some(w),
        None => graph
            .edges_directed(b, Direction::Outgoing)
            .find(|e| e.target() == a)
            .map(|e| e.weight()),
    }
}
