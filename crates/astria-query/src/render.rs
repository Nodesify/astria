//! Subgraph traversal and response rendering: BFS/DFS, text rendering,
//! token budgeting, staleness disclosure, and the shortest-path search.
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

/// `(visited nodes, observed edges, hop distance from the seeds)`.
pub(crate) type TraversalResult = (HashSet<NodeIndex>, Vec<EdgeIndex>, HashMap<NodeIndex, u32>);

pub(crate) fn bfs_subgraph(
    loaded: &LoadedGraph,
    start_nodes: &[NodeIndex],
    max_depth: usize,
    directed: bool,
    min_strength: f64,
    semantic_floor: f64,
) -> TraversalResult {
    let mut visited: HashSet<NodeIndex> = start_nodes.iter().copied().collect();
    let mut frontier: Vec<NodeIndex> = start_nodes.to_vec();
    let mut edges_seen: Vec<EdgeIndex> = Vec::new();
    let mut distance: HashMap<NodeIndex, u32> = start_nodes.iter().map(|&n| (n, 0)).collect();

    for depth in 0..max_depth {
        let mut next_frontier = Vec::new();
        for &node in &frontier {
            for (neighbor, edge_id) in
                iter_neighbors_filtered(&loaded.graph, node, directed, min_strength, semantic_floor)
            {
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    distance.insert(neighbor, depth as u32 + 1);
                    next_frontier.push(neighbor);
                    edges_seen.push(edge_id);
                }
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }
    (visited, edges_seen, distance)
}

pub(crate) fn dfs_subgraph(
    loaded: &LoadedGraph,
    start_nodes: &[NodeIndex],
    max_depth: usize,
    directed: bool,
    min_strength: f64,
    semantic_floor: f64,
) -> TraversalResult {
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut edges_seen: Vec<EdgeIndex> = Vec::new();
    let mut stack: Vec<(NodeIndex, usize)> = start_nodes.iter().rev().map(|&n| (n, 0)).collect();

    while let Some((node, depth)) = stack.pop() {
        if visited.contains(&node) || depth > max_depth {
            continue;
        }
        visited.insert(node);
        if depth == max_depth {
            continue;
        }
        for (neighbor, edge_id) in
            iter_neighbors_filtered(&loaded.graph, node, directed, min_strength, semantic_floor)
        {
            if !visited.contains(&neighbor) {
                stack.push((neighbor, depth + 1));
                edges_seen.push(edge_id);
            }
        }
    }
    (visited, edges_seen, HashMap::new())
}

/// A label that names a file ("lib.rs", "benchmark.md") rather than a symbol.
pub(crate) fn label_is_file(label: &str) -> bool {
    match label.rfind('.') {
        Some(dot) if dot > 0 => {
            let ext = &label[dot + 1..];
            !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn subgraph_to_text(
    loaded: &LoadedGraph,
    visited: &HashSet<NodeIndex>,
    edges_seen: &[EdgeIndex],
    relevance: &HashMap<NodeIndex, f64>,
    distance: &HashMap<NodeIndex, u32>,
    reach_strength: &HashMap<NodeIndex, f64>,
    prefer_files: bool,
    token_budget: i64,
    skip_records: usize,
    header: &str,
) -> astria_core::Result<(String, Option<usize>)> {
    // Relevance-ranked, not hub-ranked: question-matched seeds surface
    // first, then nodes by traversal distance to those seeds, and only
    // then by degree. Pure degree ordering buried the files the question
    // was actually about beneath graph-wide hubs.
    let mut node_list: Vec<NodeIndex> = visited.iter().copied().collect();
    node_list.sort_by(|&a, &b| {
        let na = &loaded.graph[a];
        let nb = &loaded.graph[b];
        let sa = relevance.get(&a).copied().unwrap_or(0.0);
        let sb = relevance.get(&b).copied().unwrap_or(0.0);
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            // Weakly-reached nodes (best touching edge below the semantic
            // floor) come after strongly-reached ones at equal relevance.
            // Plain graphs hold no such nodes: their edges sit at 0.7+.
            .then_with(|| {
                let wa = reach_strength.get(&a).copied().unwrap_or(1.0);
                let wb = reach_strength.get(&b).copied().unwrap_or(1.0);
                (wa < SEMANTIC_WEAK_FLOOR).cmp(&(wb < SEMANTIC_WEAK_FLOOR))
            })
            .then_with(|| {
                distance
                    .get(&a)
                    .copied()
                    .unwrap_or(u32::MAX)
                    .cmp(&distance.get(&b).copied().unwrap_or(u32::MAX))
            })
            .then_with(|| {
                loaded
                    .graph
                    .neighbors(b)
                    .count()
                    .cmp(&loaded.graph.neighbors(a).count())
            })
            .then_with(|| {
                if prefer_files {
                    let fa = label_is_file(&na.label);
                    let fb = label_is_file(&nb.label);
                    fb.cmp(&fa) // file nodes before symbols at equal relevance
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| na.label.cmp(&nb.label))
            .then_with(|| na.id.cmp(&nb.id))
    });

    let mut records = Vec::new();
    for idx in &node_list {
        let idx = *idx;
        let node = &loaded.graph[idx];
        let comm = node.community.map_or("?".to_string(), |c| c.to_string());
        let loc = match node.source_line {
            Some(line) => format!("{}:{}", loaded.display_path(&node.source_file), line),
            None => loaded.display_path(&node.source_file),
        };
        let mut line = format!(
            "NODE {} [id={} src={} community={}]\n",
            node.label, node.id, loc, comm
        );
        // Chunked bodies cite their covered line range so agents can quote
        // exact spans; harness parsers only read the src= token, so the
        // range rides on its own line.
        if node.file_type == "chunk" {
            if let (Some(start), Some(doc)) = (node.source_line, &node.docstring) {
                let end = start + doc.lines().count() as i64 - 1;
                line.push_str(&format!("  span: L{start}-L{end}\n"));
            }
        }
        if let Some(sig) = &node.signature {
            let short: String = sig.chars().take(140).collect();
            line.push_str(&format!("  sig: {}\n", short));
        } else if let Some(ref doc) = node.docstring {
            if !doc.is_empty() {
                let summary: String = doc.chars().take(200).collect();
                line.push_str(&format!("  summary: {}\n", summary));
            }
        }
        records.push(line);
    }
    let mut edge_records = Vec::new();
    let mut edge_list = edges_seen.to_vec();
    edge_list.sort_by(|&a, &b| {
        let key = |edge| {
            let (source, target) = loaded.graph.edge_endpoints(edge).unwrap();
            let score = relevance
                .get(&source)
                .copied()
                .unwrap_or(0.0)
                .max(relevance.get(&target).copied().unwrap_or(0.0));
            (
                score,
                &loaded.graph[source].id,
                &loaded.graph[target].id,
                &loaded.graph[edge].relation,
            )
        };
        let ka = key(a);
        let kb = key(b);
        kb.0.total_cmp(&ka.0)
            .then_with(|| ka.1.cmp(kb.1))
            .then_with(|| ka.2.cmp(kb.2))
            .then_with(|| ka.3.cmp(kb.3))
            .then_with(|| a.index().cmp(&b.index()))
    });
    for &edge_id in &edge_list {
        if let Some((src_idx, tgt_idx)) = loaded.graph.edge_endpoints(edge_id) {
            let src = &loaded.graph[src_idx];
            let tgt = &loaded.graph[tgt_idx];
            let edge = &loaded.graph[edge_id];
            let loc = match edge.source_line {
                Some(l) => format!(" @{}:{}", loaded.display_path(&edge.source_file), l),
                None => String::new(),
            };
            let score = edge
                .confidence_score
                .map(|s| format!(":{s:.2}"))
                .unwrap_or_default();
            let line = format!(
                "EDGE {} --{} [{}{}]--> {}{}\n",
                src.label, edge.relation, edge.confidence, score, tgt.label, loc
            );
            edge_records.push(line);
        }
    }
    // Fixed interleaving keeps relationships on the first page while the
    // cursor still addresses every complete node and edge exactly once.
    let mut interleaved = Vec::with_capacity(records.len() + edge_records.len());
    let mut nodes = records.into_iter();
    let mut edges = edge_records.into_iter();
    loop {
        let before = interleaved.len();
        interleaved.extend(nodes.by_ref().take(2));
        interleaved.extend(edges.by_ref().take(1));
        if interleaved.len() == before {
            break;
        }
    }
    render_page(header, &interleaved, skip_records, token_budget)
}

/// Public output contract: o200k_base, ordinary text (special-looking strings
/// are encoded literally). All headers, timestamps and pagination count.
pub fn count_response_tokens(text: &str) -> usize {
    tiktoken_rs::o200k_base_singleton()
        .encode_ordinary(text)
        .len()
}

/// How the working tree has drifted from the graph's source snapshot.
/// Distinct counts keep the query header honest about WHAT drifted —
/// deletions and timestamp-preserving edits used to be invisible.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct SourceDrift {
    /// Manifest files modified after publication (mtime probe).
    pub modified: usize,
    /// Manifest files that no longer exist.
    pub deleted: usize,
    /// Files whose size differs from the manifest record while the mtime
    /// looks fresh — edits that preserved (or forged) the timestamp but
    /// not the length.
    pub size_changed: usize,
}

impl SourceDrift {
    pub(crate) fn total(&self) -> usize {
        self.modified + self.deleted + self.size_changed
    }
}

/// Compare the file manifest (the graph's source snapshot) against the
/// working tree: modifications by mtime, deletions by existence, and
/// preserved-timestamp edits by size. Stat-only — this runs on every query,
/// so re-hashing content (the exact check `astria merge-gate` does at gate
/// time) is out of scope here. This measures SOURCE freshness; the build
/// timestamp in the same header carries graph AGE. `None` when the manifest
/// is unreadable — disclosure is best-effort and must never fail a query.
/// Known limit: an edit that preserves BOTH mtime and size is invisible
/// without re-hashing, and files ADDED after the build are not in the
/// manifest to probe (the merge gate's commit check covers both).
pub(crate) fn source_drift_since(
    db: &Connection,
    built_at_secs: u64,
    root: Option<&str>,
) -> Option<SourceDrift> {
    let cutoff = std::time::UNIX_EPOCH + std::time::Duration::from_secs(built_at_secs + 2);
    let mut stmt = db
        .prepare("SELECT file_path, size_bytes FROM file_manifest")
        .ok()?;
    let rows: Vec<(String, i64)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .ok()?
        .flatten()
        .collect();
    // Manifest paths are stored RELATIVE to the project root — resolve
    // them against it, or the probe silently stats nothing whenever the
    // process cwd is not the project (an MCP server's usual case).
    let resolve = |path: &str| -> std::path::PathBuf {
        let p = std::path::Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            match root {
                Some(r) => std::path::Path::new(r).join(p),
                None => p.to_path_buf(),
            }
        }
    };
    let mut drift = SourceDrift::default();
    for (path, recorded_size) in &rows {
        match std::fs::metadata(resolve(path)) {
            Err(_) => drift.deleted += 1,
            Ok(meta) => {
                if let Ok(modified) = meta.modified() {
                    if modified > cutoff {
                        drift.modified += 1;
                        continue;
                    }
                }
                if meta.len() as i64 != *recorded_size {
                    drift.size_changed += 1;
                }
            }
        }
    }
    Some(drift)
}

pub(crate) fn render_page(
    header: &str,
    records: &[String],
    cursor: usize,
    budget: i64,
) -> astria_core::Result<(String, Option<usize>)> {
    let limit = usize::try_from(budget)
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| {
            astria_core::AstriaError::Graph(
                "budget must be a positive o200k_base token count".into(),
            )
        })?;
    if cursor > records.len() {
        return Err(astria_core::AstriaError::Graph(format!(
            "cursor {cursor} exceeds {} records",
            records.len()
        )));
    }
    let mut body = String::new();
    let mut best = None;
    for end in cursor..=records.len() {
        let next = (end < records.len()).then_some(end);
        let footer = next
            .map(|n| format!("\n(continuation: re-run with cursor {n} for the next records)\n"))
            .unwrap_or_default();
        let text = format!("{header}{body}{footer}");
        if count_response_tokens(&text) > limit {
            break;
        }
        if end > cursor || end == records.len() {
            best = Some((text, next));
        }
        if let Some(record) = records.get(end) {
            body.push_str(record);
        }
    }
    // A final page has no continuation footer. Even if the footer alone
    // does not fit, the remaining complete response may still fit.
    if best.is_none() {
        let final_page = format!("{header}{}", records[cursor..].concat());
        if count_response_tokens(&final_page) <= limit {
            return Ok((final_page, None));
        }
    }
    best.ok_or_else(|| {
        astria_core::AstriaError::Graph(
            "budget too small for the response metadata and next complete record; increase budget"
                .into(),
        )
    })
}

pub(crate) fn shortest_path_bfs(
    loaded: &LoadedGraph,
    start: NodeIndex,
    end: NodeIndex,
    directed: bool,
    min_strength: f64,
    semantic_floor: f64,
) -> Option<Vec<EdgeIndex>> {
    if start == end {
        return Some(Vec::new());
    }
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut parent: HashMap<NodeIndex, (NodeIndex, EdgeIndex)> = HashMap::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(start);
    visited.insert(start);

    while let Some(current) = queue.pop_front() {
        for (neighbor, edge_id) in iter_neighbors_filtered(
            &loaded.graph,
            current,
            directed,
            min_strength,
            semantic_floor,
        ) {
            if visited.contains(&neighbor) {
                continue;
            }
            parent.insert(neighbor, (current, edge_id));
            if neighbor == end {
                let mut path = Vec::new();
                let mut cur = end;
                while let Some(&(p, edge_id)) = parent.get(&cur) {
                    path.push(edge_id);
                    cur = p;
                }
                path.reverse();
                return Some(path);
            }
            visited.insert(neighbor);
            queue.push_back(neighbor);
        }
    }
    None
}
