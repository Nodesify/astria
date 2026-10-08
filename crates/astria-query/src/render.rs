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
    } else { node.label.clone() }
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
) -> astria_core::Result<QueryResponse> {
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
        let unresolved = |n: &NodeData| n.source_file.is_empty() && matches!(n.file_type.as_str(), "stub" | "reference");
        unresolved(na).cmp(&unresolved(nb))
            .then_with(|| sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            )
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
        let mut line = if node.source_file.is_empty() && matches!(node.file_type.as_str(), "stub" | "reference") {
            format!("UNRESOLVED {label}\n")
        } else { format!(
            "NODE {} [id={} src={} community={}]\n",
            label, node.id, loc, comm
        ) };
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
            id: node.id.clone(), label, file_type: node.file_type.clone(),
            source_file: loaded.display_path(&node.source_file), source_line: node.source_line,
            community: node.community, signature: node.signature.as_ref().map(|s| s.chars().take(140).collect()),
            summary: node.docstring.as_ref().map(|s| s.chars().take(200).collect()),
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
                display_label(src), edge.relation, edge.confidence, score, display_label(tgt), loc
            );
            typed_edges.push(QueryEdge {
                source: src.id.clone(), target: tgt.id.clone(), relation: edge.relation.clone(),
                confidence: edge.confidence.clone(), confidence_score: edge.confidence_score,
                source_file: loaded.display_path(&edge.source_file), source_line: edge.source_line,
            });
            edge_records.push(line);
        }
    }
    // Fixed interleaving keeps relationships on the first page while the
    // cursor still addresses every complete node and edge exactly once.
    let mut interleaved = Vec::with_capacity(records.len() + edge_records.len());
    enum Record { Node(QueryNode), Edge(QueryEdge) }
    let mut typed = Vec::with_capacity(records.len() + edge_records.len());
    let mut nodes = records.into_iter().zip(node_records);
    let mut edges = edge_records.into_iter().zip(typed_edges);
    loop {
        let before = interleaved.len();
        for (text, node) in nodes.by_ref().take(2) { interleaved.push(text); typed.push(Record::Node(node)); }
        for (text, edge) in edges.by_ref().take(1) { interleaved.push(text); typed.push(Record::Edge(edge)); }
        if interleaved.len() == before {
            break;
        }
    }
    let (text, next_cursor) = render_page(header, &interleaved, skip_records, token_budget)?;
    let end = next_cursor.unwrap_or(typed.len());
    let mut response = QueryResponse { text, next_cursor, ..Default::default() };
    for record in typed.into_iter().skip(skip_records).take(end - skip_records) {
        match record { Record::Node(node) => response.nodes.push(node), Record::Edge(edge) => response.edges.push(edge) }
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
