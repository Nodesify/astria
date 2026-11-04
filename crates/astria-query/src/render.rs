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
    let mut distance: HashMap<NodeIndex, u32> = start_nodes.iter().map(|&n| (n, 0)).collect();

    for depth in 0..max_depth {
        let mut next_frontier = Vec::new();
        for &node in &frontier {
            for (neighbor, _) in
                iter_neighbors_filtered(&loaded.graph, node, directed, min_strength, semantic_floor)
            {
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    distance.insert(neighbor, depth as u32 + 1);
                    next_frontier.push(neighbor);
                }
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }
    // Node discovery and relationship selection are separate: a traversal
    // tree omits calls between seeds, cycles and multiple relations between
    // the same symbols. Return the qualifying induced subgraph instead.
    let edges_seen = selected_edges(loaded, &visited, min_strength, semantic_floor);
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
    let mut stack: Vec<(NodeIndex, usize)> = start_nodes.iter().rev().map(|&n| (n, 0)).collect();

    while let Some((node, depth)) = stack.pop() {
        if visited.contains(&node) || depth > max_depth {
            continue;
        }
        visited.insert(node);
        if depth == max_depth {
            continue;
        }
        for (neighbor, _) in
            iter_neighbors_filtered(&loaded.graph, node, directed, min_strength, semantic_floor)
        {
            if !visited.contains(&neighbor) {
                stack.push((neighbor, depth + 1));
            }
        }
    }
    let edges_seen = selected_edges(loaded, &visited, min_strength, semantic_floor);
    (visited, edges_seen, HashMap::new())
}

fn selected_edges(
    loaded: &LoadedGraph,
    visited: &HashSet<NodeIndex>,
    min_strength: f64,
    semantic_floor: f64,
) -> Vec<EdgeIndex> {
    let mut edges: Vec<_> = loaded
        .graph
        .edge_references()
        .filter(|edge| visited.contains(&edge.source()) && visited.contains(&edge.target()))
        .filter(|edge| edge.weight().meets_detail(min_strength))
        .filter(|edge| !below_semantic_floor(edge.weight(), semantic_floor))
        .map(|edge| edge.id())
        .collect();
    edges.sort_by_key(|edge| edge.index());
    edges.dedup();
    edges
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

pub(crate) fn display_label(node: &NodeData) -> String {
    if node.source_file.is_empty() && matches!(node.file_type.as_str(), "stub" | "reference") {
        let tail = node.label.rsplit("::").next().unwrap_or(&node.label);
        format!("{} (unresolved)", tail.chars().take(60).collect::<String>())
    } else {
        node.label.clone()
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
    structural_degree: &HashMap<NodeIndex, usize>,
    prefer_files: bool,
    token_budget: i64,
    skip_records: usize,
    header: &str,
) -> astria_core::Result<QueryResponse> {
    // Relevance-ranked, not hub-ranked: question-matched seeds surface
    // first, then nodes by traversal distance to those seeds, and only
    // then by degree. Pure degree ordering buried the files the question
    // was actually about beneath graph-wide hubs.
    let declaration = |n: &NodeData| {
        n.file_type == "code"
            && !n.source_file.is_empty()
            && n.source_line.is_some()
            && !label_is_file(&n.label)
    };
    let mut node_list: Vec<NodeIndex> = visited.iter().copied().collect();
    node_list.sort_by(|&a, &b| {
        let na = &loaded.graph[a];
        let nb = &loaded.graph[b];
        let sa = relevance.get(&a).copied().unwrap_or(0.0);
        let sb = relevance.get(&b).copied().unwrap_or(0.0);
        let unresolved = |n: &NodeData| {
            n.source_file.is_empty() && matches!(n.file_type.as_str(), "stub" | "reference")
        };
        unresolved(na)
            .cmp(&unresolved(nb))
            .then_with(|| sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal))
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
                if prefer_files {
                    std::cmp::Ordering::Equal
                } else {
                    declaration(nb).cmp(&declaration(na))
                }
            })
            .then_with(|| {
                // Structural degree only: dispatch edges (possible
                // implementations) must not win relevance ties — a
                // dispatch-hubbed file is not more on-topic than the file
                // the question's terms actually match.
                structural_degree
                    .get(&b)
                    .copied()
                    .unwrap_or(0)
                    .cmp(&structural_degree.get(&a).copied().unwrap_or(0))
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
    let mut node_records = Vec::new();
    for idx in &node_list {
        let idx = *idx;
        let node = &loaded.graph[idx];
        let comm = node.community.map_or("?".to_string(), |c| c.to_string());
        let loc = match node.source_line {
            Some(line) => format!("{}:{}", loaded.display_path(&node.source_file), line),
            None => loaded.display_path(&node.source_file),
        };
        let label = display_label(node);
        let mut line = if node.source_file.is_empty()
            && matches!(node.file_type.as_str(), "stub" | "reference")
        {
            format!("UNRESOLVED {label}\n")
        } else {
            format!(
                "NODE {} [id={} src={} community={}]\n",
                label, node.id, loc, comm
            )
        };
        let retrieval_reason = if node.source_file.is_empty() {
            "unresolved relationship endpoint".to_string()
        } else if relevance.get(&idx).copied().unwrap_or(0.0) > 0.0 {
            "query relevance (lexical or embedding evidence)".to_string()
        } else if let Some(hops) = distance.get(&idx) {
            format!("graph neighbor: {hops} hops from a seed")
        } else {
            "graph traversal neighbor".to_string()
        };
        line.push_str(&format!("  retrieval: {retrieval_reason}\n"));
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
            if let Some(lifecycle) = sig.strip_prefix("astria-document: ") {
                line.push_str(&format!("  lifecycle: {lifecycle}\n"));
            } else {
                let short: String = sig.chars().take(140).collect();
                line.push_str(&format!("  sig: {}\n", short));
            }
        }
        if node
            .signature
            .as_ref()
            .is_none_or(|s| s.starts_with("astria-document: "))
        {
            if let Some(ref doc) = node.docstring {
                if !doc.is_empty() {
                    let summary: String = doc.chars().take(200).collect();
                    line.push_str(&format!("  summary: {}\n", summary));
                }
            }
        }
        node_records.push(QueryNode {
            retrieval_reason,
            id: node.id.clone(),
            label,
            file_type: node.file_type.clone(),
            source_file: loaded.display_path(&node.source_file),
            source_line: node.source_line,
            community: node.community,
            signature: node
                .signature
                .as_ref()
                .map(|s| s.chars().take(140).collect()),
            summary: node
                .docstring
                .as_ref()
                .map(|s| s.chars().take(200).collect()),
        });
        records.push(line);
    }
    let mut edge_records = Vec::new();
    let mut typed_edges = Vec::new();
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
                display_label(src),
                edge.relation,
                edge.confidence,
                score,
                display_label(tgt),
                loc
            );
            typed_edges.push(QueryEdge {
                source: src.id.clone(),
                target: tgt.id.clone(),
                relation: edge.relation.clone(),
                confidence: edge.confidence.clone(),
                confidence_score: edge.confidence_score,
                source_file: loaded.display_path(&edge.source_file),
                source_line: edge.source_line,
            });
            edge_records.push(line);
        }
    }
    // Fixed interleaving keeps relationships on the first page while the
    // cursor still addresses every complete node and edge exactly once.
    let mut interleaved = Vec::with_capacity(records.len() + edge_records.len());
    enum Record {
        Node(QueryNode),
        Edge(QueryEdge),
    }
    let mut typed = Vec::with_capacity(records.len() + edge_records.len());
    let mut nodes = records.into_iter().zip(node_records);
    let mut edges = edge_records.into_iter().zip(typed_edges);
    loop {
        let before = interleaved.len();
        for (text, node) in nodes.by_ref().take(2) {
            interleaved.push(text);
            typed.push(Record::Node(node));
        }
        for (text, edge) in edges.by_ref().take(1) {
            interleaved.push(text);
            typed.push(Record::Edge(edge));
        }
        if interleaved.len() == before {
            break;
        }
    }
    let (text, next_cursor) = render_page(header, &interleaved, skip_records, token_budget)?;
    let end = next_cursor.unwrap_or(typed.len());
    let mut response = QueryResponse {
        text,
        next_cursor,
        ..Default::default()
    };
    for record in typed
        .into_iter()
        .skip(skip_records)
        .take(end - skip_records)
    {
        match record {
            Record::Node(node) => response.nodes.push(node),
            Record::Edge(edge) => response.edges.push(edge),
        }
    }
    Ok(response)
}

/// Public output contract: o200k_base, ordinary text (special-looking strings
/// are encoded literally). All headers, timestamps and pagination count.
pub fn count_response_tokens(text: &str) -> usize {
    tiktoken_rs::o200k_base_singleton()
        .encode_ordinary(text)
        .len()
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

/// A shortest path plus the metadata conservative rendering reports: which
/// parallel edges backed each hop, how many equal-length routes existed, and
/// which hop was weakest.
pub(crate) struct ConservativePath {
    /// The chosen route's edges in traversal order — the strongest parallel
    /// edge represents each hop.
    pub edges: Vec<EdgeIndex>,
    /// Weaker parallel evidence for each hop (same order as `edges`; entries
    /// may be empty). Kept so collapsed evidence is never silently lost.
    pub also_seen: Vec<Vec<EdgeIndex>>,
    /// How many equal-length shortest routes reach the target.
    pub routes: u64,
    /// `routes` hit the saturation cap and the true count is larger.
    pub routes_capped: bool,
    /// Index into `edges` of the route's weakest hop; `None` when the path
    /// has no hops (start == end).
    pub weakest_hop: Option<usize>,
}

/// Equal-length route counts saturate here so combinatorial graphs cannot
/// overflow the counter or produce absurd display numbers.
pub(crate) const ROUTE_CAP: u64 = 1_000_000;

/// Tolerance for matching a predecessor's weakest-hop value during
/// reconstruction: label ranks and stored scores are coarse, so an exact
/// f64 comparison could miss the arc the DP just chose.
const STRENGTH_EPSILON: f64 = 1e-9;

/// Find the shortest path, breaking length ties conservatively: among all
/// equal-length routes, the one whose weakest hop is weakest is chosen, so a
/// reported confidence can never overstate how sure the tightest connection
/// is. Parallel edges between the same pair collapse into one hop — the hop
/// is as strong as its strongest evidence, and the weaker copies ride along
/// as `also_seen` instead of being dropped.
pub(crate) fn shortest_path_conservative(
    loaded: &LoadedGraph,
    start: NodeIndex,
    end: NodeIndex,
    directed: bool,
    min_strength: f64,
    semantic_floor: f64,
) -> Option<ConservativePath> {
    if start == end {
        return Some(ConservativePath {
            edges: Vec::new(),
            also_seen: Vec::new(),
            routes: 1,
            routes_capped: false,
            weakest_hop: None,
        });
    }

    // Full BFS distances from the start: the conservative selection and the
    // equal-route count are computed over the whole shortest-path DAG, not
    // just whichever route a parent-pointer BFS happens to find first.
    let mut dist: HashMap<NodeIndex, u32> = HashMap::new();
    let mut order: Vec<NodeIndex> = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    dist.insert(start, 0);
    queue.push_back(start);
    while let Some(current) = queue.pop_front() {
        order.push(current);
        let d = dist[&current];
        for (neighbor, _) in iter_neighbors_filtered(
            &loaded.graph,
            current,
            directed,
            min_strength,
            semantic_floor,
        ) {
            if let std::collections::hash_map::Entry::Vacant(entry) = dist.entry(neighbor) {
                queue.push_back(*entry.key());
                entry.insert(d + 1);
            }
        }
    }
    let end_dist = *dist.get(&end)?;

    // Weakest achievable hop strength over all shortest routes (per node),
    // the count of those routes, and the parallel-edge evidence per arc —
    // all accumulated in one BFS-layer pass.
    let mut weakest: HashMap<NodeIndex, f64> = HashMap::new();
    let mut routes: HashMap<NodeIndex, u64> = HashMap::new();
    let mut routes_capped = false;
    let mut parallel: HashMap<(usize, usize), Vec<EdgeIndex>> = HashMap::new();
    weakest.insert(start, f64::INFINITY);
    routes.insert(start, 1);
    for &u in &order {
        let du = dist[&u];
        // Arcs beyond the target's layer can never be on a shortest route
        // to it; skipping them keeps the parallel map bounded.
        if du >= end_dist {
            continue;
        }
        let wu = weakest[&u];
        let ru = routes[&u];
        // Group this node's filtered arcs by neighbor so parallel edges
        // collapse into one hop.
        let mut arcs: HashMap<NodeIndex, Vec<EdgeIndex>> = HashMap::new();
        for (v, e) in
            iter_neighbors_filtered(&loaded.graph, u, directed, min_strength, semantic_floor)
        {
            if dist.get(&v) != Some(&(du + 1)) {
                continue;
            }
            arcs.entry(v).or_default().push(e);
        }
        for (v, mut edges) in arcs {
            // Strongest edge first: it represents the hop; the rest become
            // `also_seen` evidence at render time.
            edges.sort_by(|a, b| {
                loaded.graph[*b]
                    .strength()
                    .partial_cmp(&loaded.graph[*a].strength())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let hop_strength = loaded.graph[edges[0]].strength();
            {
                let rv = routes.entry(v).or_insert(0);
                let (sum, overflowed) = rv.overflowing_add(ru);
                if overflowed || sum > ROUTE_CAP {
                    routes_capped = true;
                    *rv = ROUTE_CAP;
                } else {
                    *rv = sum;
                }
            }
            let w = wu.min(hop_strength);
            let ew = weakest.entry(v).or_insert(f64::INFINITY);
            if w < *ew {
                *ew = w;
            }
            parallel.insert((u.index(), v.index()), edges);
        }
    }

    // Walk back from the target, at each step picking a predecessor whose
    // recurrence value reproduces the target's weakest-hop DP value — i.e.
    // an arc that lies on a most-conservative equal-length route.
    let mut edges_rev: Vec<EdgeIndex> = Vec::new();
    let mut also_rev: Vec<Vec<EdgeIndex>> = Vec::new();
    let mut cur = end;
    while cur != start {
        let d = dist[&cur];
        let w_cur = weakest[&cur];
        let mut chosen: Option<(NodeIndex, Vec<EdgeIndex>)> = None;
        // Backward view of the same filtered adjacency: predecessors of
        // `cur` are incoming edges (plus outgoing when undirected).
        let mut predecessors = loaded
            .graph
            .edges_directed(cur, Direction::Incoming)
            .filter(|e| {
                e.weight().meets_detail(min_strength)
                    && !below_semantic_floor(e.weight(), semantic_floor)
            })
            .map(|e| (e.source(), e.id()))
            .chain(
                loaded
                    .graph
                    .edges_directed(cur, Direction::Outgoing)
                    .filter(|e| {
                        !directed
                            && e.weight().meets_detail(min_strength)
                            && !below_semantic_floor(e.weight(), semantic_floor)
                    })
                    .map(|e| (e.target(), e.id())),
            );
        for (u, _) in predecessors.by_ref() {
            if dist.get(&u) != Some(&(d - 1)) {
                continue;
            }
            let Some(hop_edges) = parallel.get(&(u.index(), cur.index())) else {
                continue;
            };
            let hop_strength = loaded.graph[hop_edges[0]].strength();
            if (weakest[&u].min(hop_strength) - w_cur).abs() < STRENGTH_EPSILON {
                chosen = Some((u, hop_edges.clone()));
                break;
            }
        }
        drop(predecessors);
        let (prev, hop_edges) = chosen.expect("the DP guarantees a matching predecessor");
        edges_rev.push(hop_edges[0]);
        also_rev.push(hop_edges[1..].to_vec());
        cur = prev;
    }
    edges_rev.reverse();
    also_rev.reverse();

    let mut weakest_hop = None;
    let mut weakest_strength = f64::INFINITY;
    for (i, edge_id) in edges_rev.iter().enumerate() {
        let s = loaded.graph[*edge_id].strength();
        if s < weakest_strength {
            weakest_strength = s;
            weakest_hop = Some(i);
        }
    }

    Some(ConservativePath {
        edges: edges_rev,
        also_seen: also_rev,
        routes: routes[&end],
        routes_capped,
        weakest_hop,
    })
}
