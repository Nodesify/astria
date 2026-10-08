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
    /// Immutable lexical corpus index, built once for this generation.
    pub(crate) lexical: std::sync::OnceLock<LexicalIndex>,
    estimated_bytes: std::sync::OnceLock<usize>,
}

impl LoadedGraph {
    /// Heap estimate for graph records; excludes allocator and lexical-index overhead.
    pub(crate) fn estimated_bytes(&self) -> usize {
        *self.estimated_bytes.get_or_init(|| {
            self.graph
                .node_weights()
                .map(|n| {
                    std::mem::size_of::<NodeData>()
                        + n.id.len()
                        + n.label.len()
                        + n.file_type.len()
                        + n.source_file.len()
                        + n.docstring.as_ref().map_or(0, String::len)
                        + n.signature.as_ref().map_or(0, String::len)
                })
                .sum::<usize>()
                + self
                    .graph
                    .edge_weights()
                    .map(|e| {
                        std::mem::size_of::<EdgeData>()
                            + e.relation.len()
                            + e.confidence.len()
                            + e.source_file.len()
                    })
                    .sum::<usize>()
        })
    }
    /// Root-relative display form of a stored path.
    pub(crate) fn display_path(&self, path: &str) -> String {
        match &self.root {
            Some(root) => relative_display(path, root),
            None => path.trim_start_matches("//?/").to_string(),
        }
    }
}

/// Process-wide snapshot cache: a small LRU keyed by database path AND
/// publication generation (`_meta.graph_generation`). Each request used to
/// reload every node and edge (O(V+E) allocations); the HTTP transport
/// multiplies that per request — and with multi-project serving it also
/// thrashed, because a single-slot cache replaced the previous project's
/// graph on every alternation. A cached snapshot is reused only while the
/// generation is unchanged — one cheap scalar query per load replaces the
/// full reload, and read consistency is preserved because each snapshot
/// was loaded atomically and never mutates after construction.
///
/// Capacity trades memory for alternation latency: each entry is a full
/// node+edge snapshot (roughly O(V+E) heap), so the default of 3 bounds a
/// three-project MCP server to a few tens of MB on graphs like this repo's
/// (~5k nodes / ~21k edges) while keeping alternating projects hot.
/// `ASTRIA_SNAPSHOT_CACHE_ENTRIES` (1..=16) overrides for other shapes;
/// entries beyond capacity evict least-recently-used first.
struct SnapshotCache {
    /// Front = most recently used.
    entries: std::collections::VecDeque<(String, String, std::sync::Arc<LoadedGraph>)>,
    capacity: usize,
}

impl SnapshotCache {
    fn new() -> Self {
        SnapshotCache {
            entries: std::collections::VecDeque::new(),
            capacity: snapshot_cache_capacity(),
        }
    }

    /// Front = most recently used.
    fn get(&mut self, path: &str, generation: &str) -> Option<std::sync::Arc<LoadedGraph>> {
        if let Some(pos) = self
            .entries
            .iter()
            .position(|(p, g, _)| p == path && g == generation)
        {
            let entry = self.entries.remove(pos).unwrap();
            self.entries.push_front(entry);
            return Some(std::sync::Arc::clone(&self.entries.front().unwrap().2));
        }
        None
    }

    fn insert(&mut self, path: String, generation: String, snapshot: std::sync::Arc<LoadedGraph>) {
        // A same-key reinsert (two racing loads) replaces, not duplicates.
        self.entries
            .retain(|(p, g, _)| !(p == &path && g == &generation));
        self.entries.push_front((path, generation, snapshot));
        while self.entries.len() > self.capacity {
            self.entries.pop_back();
        }
    }
}

/// Constructed on first use (capacity reads the environment, which cannot
/// happen in a static initializer).
fn snapshot_cache() -> &'static std::sync::Mutex<SnapshotCache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<SnapshotCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(SnapshotCache::new()))
}

/// Clamp for `ASTRIA_SNAPSHOT_CACHE_ENTRIES`; a pure helper so the bounds
/// are testable without touching process environment.
fn clamp_capacity(requested: Option<usize>) -> usize {
    const FLOOR: usize = 1;
    const CEILING: usize = 16;
    const DEFAULT: usize = 3;
    requested.unwrap_or(DEFAULT).clamp(FLOOR, CEILING)
}

fn snapshot_cache_capacity() -> usize {
    clamp_capacity(
        std::env::var("ASTRIA_SNAPSHOT_CACHE_ENTRIES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok()),
    )
}

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
    // Cheap freshness probe first: when the generation stamp matches a
    // cached snapshot, share it instead of rebuilding the graph. Databases
    // without a stamp (never written by this pipeline lineage) always load
    // uncached and are never cached.
    let normalized_path = std::path::Path::new(db_path)
        .to_string_lossy()
        .replace('\\', "/");
    let generation = generation_of(db);
    if let Some(generation) = generation.as_ref() {
        // Poison recovery: entries are complete Arc snapshots keyed by
        // (path, generation), so a panicked sibling leaves no broken state.
        let mut cache = snapshot_cache()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(snapshot) = cache.get(&normalized_path, generation) {
            return Ok(snapshot);
        }
    }

    let loaded = std::sync::Arc::new(load_graph_uncached(db, db_path)?);
    if let Some(generation) = generation {
        snapshot_cache()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(normalized_path, generation, std::sync::Arc::clone(&loaded));
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
        lexical: std::sync::OnceLock::new(),
        estimated_bytes: std::sync::OnceLock::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    // The cache is process-global; serialize tests that read it.
    static CACHE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn stamped_db(path_key: &str, stamp: &str) -> Connection {
        let db = astria_core::db::open_db_in_memory().unwrap();
        db.execute_batch(&format!(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('{path_key}', '{path_key}', 'code', 'f.rs')"
        ))
        .unwrap();
        db.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('graph_generation', ?1)",
            [stamp.to_string()],
        )
        .unwrap();
        db
    }

    #[test]
    fn capacity_clamps_to_bounds() {
        assert_eq!(clamp_capacity(None), 3, "default keeps three projects hot");
        assert_eq!(clamp_capacity(Some(0)), 1);
        assert_eq!(clamp_capacity(Some(5)), 5);
        assert_eq!(clamp_capacity(Some(99)), 16);
    }

    #[test]
    fn multi_project_alternation_keeps_both_snapshots() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        // The regression: a single-slot cache replaced the previous project
        // on every alternation, forcing a full O(V+E) reload each request.
        let db_a = stamped_db("a", "1:100");
        let db_b = stamped_db("b", "1:100");
        let first_a = load_graph(&db_a, "proj/a/db.sqlite").unwrap();
        let _first_b = load_graph(&db_b, "proj/b/db.sqlite").unwrap();
        let second_a = load_graph(&db_a, "proj/a/db.sqlite").unwrap();
        assert!(
            std::sync::Arc::ptr_eq(&first_a, &second_a),
            "alternating projects must serve both from the cache"
        );
    }

    #[test]
    fn lru_evicts_least_recently_used_beyond_capacity() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let capacity = snapshot_cache_capacity();
        assert!(capacity >= 2, "test needs a multi-entry cache");
        let dbs: Vec<Connection> = (0..=capacity)
            .map(|i| stamped_db(&format!("k{i}"), "1:100"))
            .collect();
        let paths: Vec<String> = (0..=capacity)
            .map(|i| format!("proj/p{i}/db.sqlite"))
            .collect();
        // Fill to capacity with entries 0..capacity-1.
        let mut snapshots: Vec<_> = dbs
            .iter()
            .zip(&paths)
            .take(capacity)
            .map(|(db, p)| load_graph(db, p).unwrap())
            .collect();
        // Touch entry 0: it becomes most-recently-used, so entry 1 is now
        // the least-recently-used eviction candidate.
        let touched = load_graph(&dbs[0], &paths[0]).unwrap();
        assert!(std::sync::Arc::ptr_eq(&snapshots[0], &touched));
        // One more project than capacity: entry 1 evicts; the touched
        // entry 0 stays.
        snapshots.push(load_graph(&dbs[capacity], &paths[capacity]).unwrap());
        let still_cached = load_graph(&dbs[0], &paths[0]).unwrap();
        assert!(
            std::sync::Arc::ptr_eq(&snapshots[0], &still_cached),
            "the touched entry must survive the overflow insert"
        );
        let reloaded = load_graph(&dbs[1], &paths[1]).unwrap();
        assert!(
            !std::sync::Arc::ptr_eq(&snapshots[1], &reloaded),
            "least-recently-used entry must have been evicted"
        );
    }

    #[test]
    fn generation_change_invalidates_cached_snapshot() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let db = stamped_db("g", "5:500");
        let first = load_graph(&db, "proj/g/db.sqlite").unwrap();
        let same = load_graph(&db, "proj/g/db.sqlite").unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &same));
        db.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('graph_generation', '6:600')",
            [],
        )
        .unwrap();
        let next = load_graph(&db, "proj/g/db.sqlite").unwrap();
        assert!(
            !std::sync::Arc::ptr_eq(&first, &next),
            "a new publication generation must force a reload"
        );
    }
}
