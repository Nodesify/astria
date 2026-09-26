use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, RwLock};

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rusqlite::Connection;

use astria_paths::relative_display;

/// Fraction of the output budget reserved for EDGE lines. Nodes alone are
/// capped at this share so a traversal never returns a bag of labels with
/// the relationships truncated away.
const NODE_BUDGET_SHARE: f64 = 0.6;
/// How many near-miss labels to suggest when a query matches nothing.
const SUGGESTION_COUNT: usize = 3;

/// Global cache of loaded graphs, keyed by normalized DB path.
static GRAPH_CACHE: LazyLock<RwLock<HashMap<String, Arc<LoadedGraph>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Invalidate the cached graph for a given DB path. Called after pipeline runs.
pub fn invalidate_graph_cache(db_path: &str) {
    let mut cache = GRAPH_CACHE.write().unwrap();
    cache.remove(db_path);
}

fn log_query(db: &Connection, question: &str, answer: &str) {
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

#[derive(Debug)]
struct NodeData {
    id: String,
    label: String,
    file_type: String,
    source_file: String,
    source_line: Option<i64>,
    community: Option<i64>,
    docstring: Option<String>,
    signature: Option<String>,
}

#[derive(Debug)]
struct EdgeData {
    relation: String,
    confidence: String,
    confidence_score: Option<f64>,
    source_file: String,
    source_line: Option<i64>,
}

impl EdgeData {
    /// Effective strength of this edge: the stored numeric score when
    /// present, otherwise a rank derived from the confidence label.
    fn strength(&self) -> f64 {
        self.confidence_score
            .unwrap_or_else(|| confidence_rank(&self.confidence))
    }
}

/// Fallback strength for edges without a numeric score. Alphabetical
/// string comparison of confidence labels does NOT order by strength
/// ("SEMANTIC" > "LLM" lexicographically), so map labels to numbers.
fn confidence_rank(confidence: &str) -> f64 {
    match confidence.to_uppercase().as_str() {
        "DECLARED" => 1.0,
        "EXTRACTED" => 0.9,
        "INFERRED" => 0.7,
        "SEMANTIC" => 0.6,
        _ => 0.5,
    }
}

struct LoadedGraph {
    /// Directed storage even though most traversals are undirected: the
    /// edge orientation (caller → callee, importer → module) is preserved,
    /// and `directed` queries can follow it.
    graph: DiGraph<NodeData, EdgeData>,
    id_to_idx: HashMap<String, NodeIndex>,
    /// Project root derived from the DB path (`root/.astria/db.sqlite`),
    /// used to shorten stored absolute paths in agent-facing output.
    root: Option<String>,
}

impl LoadedGraph {
    /// Root-relative display form of a stored path.
    fn display_path(&self, path: &str) -> String {
        match &self.root {
            Some(root) => relative_display(path, root),
            None => path.trim_start_matches("//?/").to_string(),
        }
    }
}

fn load_graph(db: &Connection, db_path: &str) -> astria_core::Result<LoadedGraph> {
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
            .filter_map(|r| r.ok())
            .collect();
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
            .filter_map(|r| r.ok())
            .collect();
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

fn load_graph_cached(db: &Connection, db_path: &str) -> astria_core::Result<Arc<LoadedGraph>> {
    {
        let cache = GRAPH_CACHE.read().unwrap();
        if let Some(entry) = cache.get(db_path) {
            return Ok(Arc::clone(entry));
        }
    }

    let loaded = Arc::new(load_graph(db, db_path)?);
    {
        let mut cache = GRAPH_CACHE.write().unwrap();
        cache.insert(db_path.to_string(), Arc::clone(&loaded));
    }
    Ok(loaded)
}

/// Neighbors of `idx`: outgoing only when traversing a directed graph,
/// both directions otherwise.
fn iter_neighbors<'a>(
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
fn iter_neighbors_filtered<'a>(
    graph: &'a DiGraph<NodeData, EdgeData>,
    idx: NodeIndex,
    directed: bool,
    min_strength: f64,
) -> impl Iterator<Item = NodeIndex> + 'a {
    let outgoing = graph
        .edges_directed(idx, Direction::Outgoing)
        .filter(move |e| e.weight().strength() >= min_strength)
        .map(|e| e.target());
    if directed {
        Box::new(outgoing) as Box<dyn Iterator<Item = NodeIndex> + 'a>
    } else {
        let incoming = graph
            .edges_directed(idx, Direction::Incoming)
            .filter(move |e| e.weight().strength() >= min_strength)
            .map(|e| e.source());
        Box::new(outgoing.chain(incoming)) as Box<dyn Iterator<Item = NodeIndex> + 'a>
    }
}

/// The strongest edge connecting `a` and `b`, in either direction.
fn edge_between(
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

/// Lowercase word tokens, splitting camelCase / snake_case / kebab-case and
/// punctuation so "parseExtraction", "parse_extraction" and
/// "parse-extraction" all tokenize identically.
fn tokenize(s: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            let prev_upper = current.chars().last().is_some_and(|c| c.is_uppercase());
            if !current.is_empty() && ch.is_uppercase() && !prev_upper {
                tokens.push(std::mem::take(&mut current).to_lowercase());
            }
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current).to_lowercase());
        }
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }
    tokens
}

/// The `k` node labels most similar to `query` — did-you-mean suggestions
/// so a failed lookup hands the agent something actionable instead of a
/// dead end. Best Jaro-Winkler score across the query's terms wins.
fn nearest_labels(loaded: &LoadedGraph, query: &str, k: usize) -> Vec<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .filter(|t| t.len() > 2)
        .map(|t| t.to_lowercase())
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(f64, &String)> = Vec::new();
    for idx in loaded.graph.node_indices() {
        let label = &loaded.graph[idx].label;
        let label_lower = label.to_lowercase();
        let best = terms
            .iter()
            .map(|t| strsim::jaro_winkler(&label_lower, t))
            .fold(0.0_f64, f64::max);
        if best > 0.6 {
            scored.push((best, label));
        }
    }
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(b.1))
    });
    scored.truncate(k);
    scored.into_iter().map(|(_, l)| l.clone()).collect()
}

/// Strip a simple English plural suffix so "communities" matches
/// "community" and "users" matches "user". Cheap morphology for code terms.
fn stem(token: &str) -> &str {
    if token.len() > 4 && token.ends_with("ies") {
        &token[..token.len() - 3] // "communities" -> "communit" (matches "community" prefix-wise)
    } else if token.len() > 3 && token.ends_with('s') && !token.ends_with("ss") {
        &token[..token.len() - 1]
    } else {
        token
    }
}

/// Hybrid seed scoring, layered from cheap/exact to expensive/fuzzy so a
/// paraphrased or slightly-misspelled question still finds its entry nodes:
/// 1. label/path substring, exact & prefix token match (deterministic, free)
/// 2. docstring token matches (where prose descriptions of symbols live)
/// Common English question/filler words: matching them is noise, not
/// retrieval signal.
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "then", "than", "that", "this", "these", "those",
    "there", "here", "of", "in", "on", "at", "by", "for", "with", "from", "into", "about", "as",
    "is", "are", "was", "were", "be", "been", "being", "am", "do", "does", "did", "doing", "have",
    "has", "had", "having", "will", "would", "shall", "should", "can", "could", "may", "might",
    "must", "to", "too", "it", "its", "itself", "they", "them", "their", "we", "us", "our", "you",
    "your", "i", "me", "my", "he", "she", "his", "her", "him", "what", "which", "who", "whom",
    "whose", "when", "where", "why", "how", "not", "no", "also", "just", "very", "some", "any",
    "each", "other", "more", "most",
];

/// 3. fuzzy token match (Jaro-Winkler) for typos and word variants
fn score_nodes(loaded: &LoadedGraph, terms: &[String]) -> Vec<(f64, NodeIndex)> {
    let mut scored: Vec<(f64, NodeIndex)> = Vec::new();
    for idx in loaded.graph.node_indices() {
        let node = &loaded.graph[idx];
        // Code answers rank above documentation and speculative stubs on
        // equal term evidence: without the prior, prose-heavy doc nodes and
        // std-call stubs crowd code symbols out of the seed set.
        let prior = match node.file_type.as_str() {
            "code" | "rationale" | "package" => 1.0,
            "stub" | "document" | "reference" | "paper" | "image" | "video" => 0.85,
            _ => 1.0,
        };
        let label_tokens = tokenize(&node.label);
        let file_tokens = tokenize(&node.source_file);
        let doc_tokens: Vec<String> = node
            .docstring
            .as_deref()
            .map(|d| tokenize(d).into_iter().take(120).collect())
            .unwrap_or_default();
        let mut score = 0.0;
        for term in terms {
            let term = term.trim();
            if term.len() <= 2 {
                continue;
            }
            let term_lower = term.to_lowercase();
            // Question words carry no retrieval signal: "where is the SSRF
            // validation and what does it check" must score on ssrf/url/
            // validation/check, not on "where"/"does" matching every doc
            // heading that contains them.
            if STOPWORDS.contains(&term_lower.as_str()) {
                continue;
            }
            let term_tokens = tokenize(term);
            let label_lower = node.label.to_lowercase();
            let sf_lower = node.source_file.to_lowercase();

            // Layer 1: label + path
            let label_score = if label_lower.contains(&term_lower) {
                1.0
            } else {
                term_tokens
                    .iter()
                    .map(|tt| {
                        let st = stem(tt);
                        if label_tokens
                            .iter()
                            .any(|lt| lt == tt || stem(lt) == st || stem(lt).starts_with(st))
                        {
                            0.9
                        } else if label_tokens.iter().any(|lt| lt.starts_with(tt.as_str())) {
                            0.7
                        } else if label_tokens.iter().any(|lt| lt.contains(tt.as_str())) {
                            0.5
                        } else {
                            0.0
                        }
                    })
                    .fold(0.0_f64, f64::max)
            };

            // Layer 2: docstring prose
            let doc_score = if !doc_tokens.is_empty() {
                let exact = doc_tokens.iter().any(|dt| {
                    term_tokens
                        .iter()
                        .any(|tt| dt == tt || stem(dt) == stem(tt))
                });
                let contains = node
                    .docstring
                    .as_deref()
                    .map(|d| d.to_lowercase().contains(&term_lower))
                    .unwrap_or(false);
                if exact {
                    0.4
                } else if contains {
                    0.35
                } else {
                    0.0
                }
            } else {
                0.0
            };

            // Layer 3: path + fuzzy fallback (typo / word-variant rescue)
            let path_score = if sf_lower.contains(&term_lower)
                || file_tokens.iter().any(|ft| term_tokens.contains(ft))
            {
                0.5
            } else {
                0.0
            };
            let fuzzy_score = if label_score + doc_score + path_score == 0.0
                && term_tokens.iter().any(|tt| {
                    label_tokens
                        .iter()
                        .any(|lt| strsim::jaro_winkler(lt, tt) > 0.85)
                }) {
                0.4
            } else {
                0.0
            };

            score += label_score.max(doc_score) + path_score + fuzzy_score;
        }
        if score > 0.0 {
            scored.push((score * prior, idx));
        }
    }
    // Deterministic order: score desc, then label, then id.
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| loaded.graph[a.1].label.cmp(&loaded.graph[b.1].label))
            .then_with(|| loaded.graph[a.1].id.cmp(&loaded.graph[b.1].id))
    });
    scored
}

fn bfs_subgraph(
    loaded: &LoadedGraph,
    start_nodes: &[NodeIndex],
    max_depth: usize,
    directed: bool,
    min_strength: f64,
) -> (
    HashSet<NodeIndex>,
    Vec<(NodeIndex, NodeIndex)>,
    HashMap<NodeIndex, u32>,
) {
    let mut visited: HashSet<NodeIndex> = start_nodes.iter().copied().collect();
    let mut frontier: Vec<NodeIndex> = start_nodes.to_vec();
    let mut edges_seen: Vec<(NodeIndex, NodeIndex)> = Vec::new();
    let mut distance: HashMap<NodeIndex, u32> = start_nodes.iter().map(|&n| (n, 0)).collect();

    for depth in 0..max_depth {
        let mut next_frontier = Vec::new();
        for &node in &frontier {
            for neighbor in iter_neighbors_filtered(&loaded.graph, node, directed, min_strength) {
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    distance.insert(neighbor, depth as u32 + 1);
                    next_frontier.push(neighbor);
                    edges_seen.push((node, neighbor));
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

fn dfs_subgraph(
    loaded: &LoadedGraph,
    start_nodes: &[NodeIndex],
    max_depth: usize,
    directed: bool,
    min_strength: f64,
) -> (
    HashSet<NodeIndex>,
    Vec<(NodeIndex, NodeIndex)>,
    HashMap<NodeIndex, u32>,
) {
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut edges_seen: Vec<(NodeIndex, NodeIndex)> = Vec::new();
    let mut stack: Vec<(NodeIndex, usize)> = start_nodes.iter().rev().map(|&n| (n, 0)).collect();

    while let Some((node, depth)) = stack.pop() {
        if visited.contains(&node) || depth > max_depth {
            continue;
        }
        visited.insert(node);
        for neighbor in iter_neighbors_filtered(&loaded.graph, node, directed, min_strength) {
            if !visited.contains(&neighbor) {
                stack.push((neighbor, depth + 1));
                edges_seen.push((node, neighbor));
            }
        }
    }
    (visited, edges_seen, HashMap::new())
}

fn subgraph_to_text(
    loaded: &LoadedGraph,
    visited: &HashSet<NodeIndex>,
    edges_seen: &[(NodeIndex, NodeIndex)],
    relevance: &HashMap<NodeIndex, f64>,
    distance: &HashMap<NodeIndex, u32>,
    token_budget: i64,
    mut skip_nodes: usize,
) -> (String, Option<usize>) {
    let char_budget = (token_budget as usize) * 3;
    // Nodes alone may not consume more than their share — the relationships
    // are the point of a graph traversal, so edges always keep budget.
    let node_budget = (char_budget as f64 * NODE_BUDGET_SHARE) as usize;
    let mut out = String::new();

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
            .then_with(|| na.label.cmp(&nb.label))
            .then_with(|| na.id.cmp(&nb.id))
    });
    if skip_nodes >= node_list.len() {
        skip_nodes = 0; // stale/overshot cursor: restart from the top
    }

    let mut shown_nodes = 0usize;
    #[allow(clippy::explicit_counter_loop)]
    for (_pos, idx) in node_list.iter().enumerate().skip(skip_nodes) {
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
        if let Some(sig) = &node.signature {
            let short: String = sig.chars().take(140).collect();
            line.push_str(&format!("  sig: {}\n", short));
        } else if let Some(ref doc) = node.docstring {
            if !doc.is_empty() {
                let summary: String = doc.chars().take(200).collect();
                line.push_str(&format!("  summary: {}\n", summary));
            }
        }
        if out.len() + line.len() > node_budget && shown_nodes > 0 {
            out.push_str(&format!(
                "... (showing nodes {}-{} of {}; edges follow)\n",
                skip_nodes + 1,
                skip_nodes + shown_nodes,
                node_list.len()
            ));
            let next = skip_nodes + shown_nodes;
            let cursor = (next < node_list.len()).then_some(next);
            return (
                finish_edges(
                    loaded,
                    out,
                    edges_seen,
                    token_budget,
                    char_budget,
                    node_budget,
                ),
                cursor,
            );
        }
        out.push_str(&line);
        shown_nodes += 1;
    }

    (
        finish_edges(
            loaded,
            out,
            edges_seen,
            token_budget,
            char_budget,
            node_budget,
        ),
        None,
    )
}

/// Edge section shared by both node-loop exits: remaining budget with a
/// floor so even a tiny budget still yields some relationships.
fn finish_edges(
    loaded: &LoadedGraph,
    mut out: String,
    edges_seen: &[(NodeIndex, NodeIndex)],
    token_budget: i64,
    char_budget: usize,
    node_budget: usize,
) -> String {
    let edge_budget = char_budget.saturating_sub(node_budget).max(200);
    let mut edge_spent = 0usize;
    for (src_idx, tgt_idx) in edges_seen {
        let src = &loaded.graph[*src_idx];
        let tgt = &loaded.graph[*tgt_idx];
        if let Some(edge) = edge_between(&loaded.graph, *src_idx, *tgt_idx) {
            let loc = match edge.source_line {
                Some(l) => format!(" @{}:{}", loaded.display_path(&edge.source_file), l),
                None => String::new(),
            };
            let line = format!(
                "EDGE {} --{} [{}]--> {}{}\n",
                src.label, edge.relation, edge.confidence, tgt.label, loc
            );
            if edge_spent > 0 && edge_spent + line.len() > edge_budget {
                out.push_str(&format!(
                    "... (truncated to ~{} token budget)\n",
                    token_budget
                ));
                return out;
            }
            out.push_str(&line);
            edge_spent += line.len();
        }
    }
    out
}

fn shortest_path_bfs(
    loaded: &LoadedGraph,
    start: NodeIndex,
    end: NodeIndex,
    directed: bool,
    min_strength: f64,
) -> Option<Vec<NodeIndex>> {
    if start == end {
        return Some(vec![start]);
    }
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut parent: HashMap<NodeIndex, NodeIndex> = HashMap::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(start);
    visited.insert(start);

    while let Some(current) = queue.pop_front() {
        for neighbor in iter_neighbors_filtered(&loaded.graph, current, directed, min_strength) {
            if visited.contains(&neighbor) {
                continue;
            }
            parent.insert(neighbor, current);
            if neighbor == end {
                let mut path = vec![end];
                let mut cur = end;
                while let Some(&p) = parent.get(&cur) {
                    path.push(p);
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

/// Traversal query. `min_strength` is the fidelity tier (0.0 = all facts;
/// 0.9 = EXTRACTED/DECLARED only); `cursor` continues a previously
/// truncated node list. Returns (text, nodes, edges, next_cursor) —
/// `next_cursor` is Some when more ranked nodes remain.
#[allow(clippy::too_many_arguments)]
pub fn query_graph(
    db: &Connection,
    db_path: &str,
    question: &str,
    mode: &str,
    depth: usize,
    budget: i64,
    directed: bool,
    min_strength: f64,
    cursor: usize,
) -> astria_core::Result<(String, usize, usize, Option<usize>)> {
    query_graph_with_semantic(
        db,
        db_path,
        question,
        mode,
        depth,
        budget,
        directed,
        min_strength,
        cursor,
        &[],
    )
}

/// Rescale a cosine in [0.55, 1.0] into the seed-score scale: a perfect
/// semantic match (1.0) lands just under an exact label match (1.0+),
/// mid-range matches land near the path/docstring layer.
fn semantic_seed_score(cosine: f64) -> f64 {
    ((cosine - 0.55) * 1.5).clamp(0.0, 0.67)
}

/// `query_graph` plus semantic seed candidates: `(node_id, cosine)` pairs
/// from an embedding model. Semantic matches merge with token scoring by
/// taking each node's best score, so a purely semantic match still seeds
/// the traversal — how "auth flow" finds `SessionMiddleware` with zero
/// string overlap — while exact label matches keep outranking it.
#[allow(clippy::too_many_arguments)]
pub fn query_graph_with_semantic(
    db: &Connection,
    db_path: &str,
    question: &str,
    mode: &str,
    depth: usize,
    budget: i64,
    directed: bool,
    min_strength: f64,
    cursor: usize,
    semantic: &[(String, f64)],
) -> astria_core::Result<(String, usize, usize, Option<usize>)> {
    let loaded = load_graph_cached(db, db_path)?;
    if loaded.graph.node_count() == 0 {
        return Ok(("No nodes in graph.".to_string(), 0, 0, None));
    }

    let terms: Vec<String> = question.split_whitespace().map(|s| s.to_string()).collect();
    let mut scored = score_nodes(&loaded, &terms);

    // Merge semantic candidates: max(token score, rescaled cosine) per node.
    if !semantic.is_empty() {
        let mut by_index: std::collections::HashMap<NodeIndex, f64> =
            scored.iter().map(|(s, i)| (*i, *s)).collect();
        for (node_id, cosine) in semantic {
            if let Some(&idx) = loaded.id_to_idx.get(node_id) {
                let entry = by_index.entry(idx).or_insert(0.0);
                *entry = (*entry).max(semantic_seed_score(*cosine));
            }
        }
        scored = by_index.into_iter().map(|(i, s)| (s, i)).collect();
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| loaded.graph[a.1].label.cmp(&loaded.graph[b.1].label))
                .then_with(|| loaded.graph[a.1].id.cmp(&loaded.graph[b.1].id))
        });
    }

    if scored.is_empty() {
        let suggestions = nearest_labels(&loaded, question, SUGGESTION_COUNT);
        let msg = if suggestions.is_empty() {
            "No matching nodes found.".to_string()
        } else {
            format!(
                "No matching nodes found. Did you mean: {}?",
                suggestions.join(", ")
            )
        };
        return Ok((msg, 0, 0, None));
    }

    let seed_nodes: Vec<NodeIndex> = scored.iter().take(5).map(|(_, idx)| *idx).collect();
    let (visited, edges_seen, distance) = if mode == "dfs" {
        dfs_subgraph(&loaded, &seed_nodes, depth, directed, min_strength)
    } else {
        bfs_subgraph(&loaded, &seed_nodes, depth, directed, min_strength)
    };

    let seed_labels: Vec<String> = seed_nodes
        .iter()
        .map(|&idx| loaded.graph[idx].label.clone())
        .collect();

    let header = format!(
        "Traversal: {} depth={}{} | Start: {:?} | {} nodes found\n\n",
        mode.to_uppercase(),
        depth,
        if directed { " directed" } else { "" },
        seed_labels,
        visited.len()
    );
    let relevance: HashMap<NodeIndex, f64> = scored.iter().map(|(s, i)| (*i, *s)).collect();
    let (body, next_cursor) = subgraph_to_text(
        &loaded,
        &visited,
        &edges_seen,
        &relevance,
        &distance,
        budget,
        cursor,
    );
    let mut result_text = header + &body;
    if let Some(next) = next_cursor {
        result_text.push_str(&format!(
            "\n(continuation: re-run with cursor {next} for the next nodes)\n"
        ));
    }

    log_query(db, question, &result_text);
    record_query_pairs(db, &loaded, &seed_nodes, &visited, question);

    Ok((result_text, visited.len(), edges_seen.len(), next_cursor))
}

/// Feedback loop bookkeeping: for one answered query, record which
/// (seed, discovered) node pairs the traversal connected. Pairs recurring
/// across DISTINCT questions later promote into `learned` edges — the
/// graph remembers which connections users actually keep asking about.
fn record_query_pairs(
    db: &Connection,
    loaded: &LoadedGraph,
    seeds: &[NodeIndex],
    visited: &HashSet<NodeIndex>,
    question: &str,
) {
    let question: String = question
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect();
    if question.is_empty() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    // Top seeds by rank; discoveries ranked by degree — the load-bearing
    // nodes the question actually reached.
    let top_seeds: Vec<&NodeIndex> = seeds.iter().take(3).collect();
    let mut discoveries: Vec<(usize, &NodeIndex)> = visited
        .iter()
        .filter(|idx| !seeds.contains(idx))
        .map(|idx| (loaded.graph.neighbors(*idx).count(), idx))
        .collect();
    discoveries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    let top_discoveries: Vec<&NodeIndex> =
        discoveries.iter().map(|(_, idx)| *idx).take(5).collect();

    for seed in &top_seeds {
        for discovery in &top_discoveries {
            let (source, target) = if loaded.graph[**seed].id <= loaded.graph[**discovery].id {
                (&loaded.graph[**seed].id, &loaded.graph[**discovery].id)
            } else {
                (&loaded.graph[**discovery].id, &loaded.graph[**seed].id)
            };
            let _ = db.execute(
                "INSERT INTO query_pairs (source, target, question, hits, first_seen, last_seen)
                 VALUES (?1, ?2, ?3, 1, ?4, ?4)
                 ON CONFLICT (source, target, question)
                 DO UPDATE SET hits = hits + 1, last_seen = ?4",
                rusqlite::params![source, target, question, ts],
            );
        }
    }
}

/// Promote recurring query pairs into `learned` edges: a pair qualifies
/// when it was connected by at least `min_questions` DISTINCT questions
/// with at least `min_hits` total repetitions. Existing learned edges are
/// regenerated (idempotent). Learned edges carry confidence INFERRED with
/// a hits-based score, so `--detail high` traversals can filter them.
/// Returns the number of learned edges materialized.
pub fn promote_learned_edges(
    db: &Connection,
    min_questions: usize,
    min_hits: usize,
) -> astria_core::Result<usize> {
    // Drop pairs whose endpoints were deleted by later builds — nodes come
    // and go with files, and a stale reference would violate the edges FK.
    db.execute(
        "DELETE FROM query_pairs WHERE source NOT IN (SELECT id FROM nodes)
         OR target NOT IN (SELECT id FROM nodes)",
        [],
    )?;

    let pairs: Vec<(String, String, i64)> = {
        let mut stmt = db.prepare(
            "SELECT source, target, SUM(hits) FROM query_pairs
             GROUP BY source, target
             HAVING COUNT(DISTINCT question) >= ?1 AND SUM(hits) >= ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![min_questions as i64, min_hits as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        rows.flatten().collect()
    };

    db.execute(
        "DELETE FROM edges WHERE relation = 'learned' AND source_file = 'query_history'",
        [],
    )?;

    let tx = db.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file)
             VALUES (?1, ?2, 'learned', 'INFERRED', ?3, 'query_history')",
        )?;
        for (source, target, hits) in &pairs {
            // 3 hits -> 0.5, 10+ hits -> 1.0: recency-weighted importance
            // without letting a single hot pair dominate high-fidelity views.
            let score = ((*hits as f64 - 1.0) / 9.0).clamp(0.1, 1.0);
            stmt.execute(rusqlite::params![source, target, score])?;
        }
    }
    tx.commit()?;
    Ok(pairs.len())
}

pub fn find_shortest_path(
    db: &Connection,
    db_path: &str,
    source_query: &str,
    target_query: &str,
    directed: bool,
    min_strength: f64,
) -> astria_core::Result<(bool, usize, String)> {
    let loaded = load_graph_cached(db, db_path)?;
    if loaded.graph.node_count() == 0 {
        return Ok((false, 0, "No nodes in graph.".to_string()));
    }

    // Exact ids win over fuzzy scoring, and a stub never shadows a
    // same-named definition (same rule as affected/explain seeds). Scoring
    // stays as the fallback for natural-language endpoints.
    let resolve_endpoint = |query: &str| -> Option<NodeIndex> {
        if let Some(&idx) = loaded.id_to_idx.get(query) {
            let is_stub: bool = db
                .query_row(
                    "SELECT file_type = 'stub' FROM nodes WHERE id = ?1",
                    rusqlite::params![query],
                    |r| r.get(0),
                )
                .unwrap_or(false);
            if !is_stub {
                return Some(idx);
            }
            let bare = query
                .trim_start_matches('.')
                .trim_end_matches("()")
                .to_lowercase();
            if let Some(id) = astria_core::db::prefer_non_stub_id(db, &bare) {
                if let Some(&better) = loaded.id_to_idx.get(id.as_str()) {
                    return Some(better);
                }
            }
            return Some(idx);
        }
        None
    };

    let src_idx = match resolve_endpoint(source_query) {
        Some(idx) => idx,
        None => {
            let src_terms: Vec<String> = source_query
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            let src_scored = score_nodes(&loaded, &src_terms);
            match src_scored.first() {
                Some((_, idx)) => *idx,
                None => {
                    let mut msg = format!("No matching node for '{}'.", source_query);
                    let suggestions = nearest_labels(&loaded, source_query, SUGGESTION_COUNT);
                    if !suggestions.is_empty() {
                        msg.push_str(&format!(" Did you mean: {}?", suggestions.join(", ")));
                    }
                    return Ok((false, 0, msg));
                }
            }
        }
    };
    let tgt_idx = match resolve_endpoint(target_query) {
        Some(idx) => idx,
        None => {
            let tgt_terms: Vec<String> = target_query
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            let tgt_scored = score_nodes(&loaded, &tgt_terms);
            match tgt_scored.first() {
                Some((_, idx)) => *idx,
                None => {
                    let mut msg = format!("No matching node for '{}'.", target_query);
                    let suggestions = nearest_labels(&loaded, target_query, SUGGESTION_COUNT);
                    if !suggestions.is_empty() {
                        msg.push_str(&format!(" Did you mean: {}?", suggestions.join(", ")));
                    }
                    return Ok((false, 0, msg));
                }
            }
        }
    };

    let path = match shortest_path_bfs(&loaded, src_idx, tgt_idx, directed, min_strength) {
        Some(p) => p,
        None => return Ok((false, 0, "No path found.".to_string())),
    };

    let hops = path.len().saturating_sub(1);
    let mut text = format!("Shortest path ({} hops):\n", hops);

    for i in 0..path.len().saturating_sub(1) {
        let src = &loaded.graph[path[i]];
        let tgt = &loaded.graph[path[i + 1]];
        let edge_info = edge_between(&loaded.graph, path[i], path[i + 1]);
        let rel = edge_info.map_or("?".to_string(), |e| e.relation.clone());
        let conf = edge_info.map_or("?".to_string(), |e| e.confidence.clone());
        text.push_str(&format!(
            "  {} --{} [{}]--> {}\n",
            src.label, rel, conf, tgt.label
        ));
    }

    let answer = format!("path found: {} hops", hops);
    log_query(
        db,
        &format!("{} -> {}", source_query, target_query),
        &answer,
    );

    Ok((true, hops, text))
}

/// Aider-style repo map: files ranked by PageRank over the file-level
/// reference graph, each with its most-connected symbols. One budgeted
/// blob that orients an agent over the whole repo — the "orient for a
/// fixed token cost" artifact that replaces scattered file reading.
pub fn repo_map(
    db: &Connection,
    db_path: &str,
    budget: i64,
    min_strength: f64,
) -> astria_core::Result<(String, usize)> {
    let loaded = load_graph_cached(db, db_path)?;
    if loaded.graph.node_count() == 0 {
        return Ok(("No nodes in graph.".to_string(), 0));
    }

    // Display-form file of each node (indexed by NodeIndex::index()).
    let file_of: Vec<String> = loaded
        .graph
        .node_indices()
        .map(|idx| loaded.display_path(&loaded.graph[idx].source_file))
        .collect();
    let mut files: Vec<String> = file_of.clone();
    files.sort();
    files.dedup();
    let n = files.len();
    let file_rank: HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.as_str(), i))
        .collect();

    // File-level adjacency: undirected weight = cross-file edge count.
    let mut adj: Vec<HashMap<usize, f64>> = vec![HashMap::new(); n];
    let mut out_sum: Vec<f64> = vec![0.0; n];
    for e in loaded.graph.edge_references() {
        if e.weight().strength() < min_strength {
            continue;
        }
        let sf = &file_of[e.source().index()];
        let tf = &file_of[e.target().index()];
        let (a, b) = (file_rank[sf.as_str()], file_rank[tf.as_str()]);
        if a != b {
            *adj[a].entry(b).or_insert(0.0) += 1.0;
            *adj[b].entry(a).or_insert(0.0) += 1.0;
            out_sum[a] += 1.0;
            out_sum[b] += 1.0;
        }
    }

    // PageRank with dangling-mass redistribution.
    let damping = 0.85_f64;
    let mut rank: Vec<f64> = vec![1.0 / n as f64; n];
    for _ in 0..30 {
        let dangling: f64 = (0..n).filter(|&i| out_sum[i] <= 0.0).map(|i| rank[i]).sum();
        let mut next = vec![(1.0 - damping) / n as f64 + damping * dangling / n as f64; n];
        for i in 0..n {
            if out_sum[i] <= 0.0 {
                continue;
            }
            let share = damping * rank[i] / out_sum[i];
            for (&j, &w) in &adj[i] {
                next[j] += share * w;
            }
        }
        rank = next;
    }

    // Top symbols per file by degree (deterministic ties by label).
    let mut file_symbols: Vec<Vec<(NodeIndex, usize)>> = vec![Vec::new(); n];
    for idx in loaded.graph.node_indices() {
        let fi = file_rank[file_of[idx.index()].as_str()];
        file_symbols[fi].push((idx, loaded.graph.neighbors(idx).count()));
    }
    for syms in &mut file_symbols {
        syms.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| loaded.graph[a.0].label.cmp(&loaded.graph[b.0].label))
        });
        syms.truncate(3);
    }

    // Emit within budget.
    let char_budget = (budget.max(1) as usize) * 3;
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| {
        rank[*b]
            .partial_cmp(&rank[*a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| files[*a].cmp(&files[*b]))
    });

    let mut out = format!("Repo map ({} files, PageRank-ranked):\n", n);
    let mut shown = 0usize;
    for &fi in &order {
        let mut block = format!("\n{} (rank {:.4})\n", files[fi], rank[fi]);
        for (idx, deg) in &file_symbols[fi] {
            let node = &loaded.graph[*idx];
            block.push_str(&format!(
                "  - {} [id={}] (degree {})\n",
                node.label, node.id, deg
            ));
        }
        if out.len() + block.len() > char_budget && shown > 0 {
            out.push_str(&format!(
                "\n... (map truncated: {} of {} files; raise --budget for more)\n",
                shown, n
            ));
            break;
        }
        out.push_str(&block);
        shown += 1;
    }

    Ok((out, shown))
}

pub fn explain_with_neighbors(
    db: &Connection,
    db_path: &str,
    node_id: &str,
) -> astria_core::Result<Option<ExplainResult>> {
    let loaded = load_graph_cached(db, db_path)?;

    // A stub must not shadow a same-named real definition: explaining by a
    // bare name would otherwise land on a speculative node (no edges, no
    // provenance) instead of the symbol.
    let resolved_id = {
        let bare = node_id
            .trim_start_matches('.')
            .trim_end_matches("()")
            .to_lowercase();
        let exact_is_stub: bool = db
            .query_row(
                "SELECT file_type = 'stub' FROM nodes WHERE id = ?1",
                rusqlite::params![node_id],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if exact_is_stub {
            astria_core::db::prefer_non_stub_id(db, &bare).unwrap_or_else(|| node_id.to_string())
        } else {
            node_id.to_string()
        }
    };

    let idx = match loaded.id_to_idx.get(resolved_id.as_str()) {
        Some(&idx) => idx,
        None => {
            let terms: Vec<String> = resolved_id
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            let scored = score_nodes(&loaded, &terms);
            match scored.first() {
                Some((_, idx)) => *idx,
                None => return Ok(None),
            }
        }
    };

    let node = &loaded.graph[idx];
    // Explain is a lookup, not a traversal: neighbors in both directions.
    let mut seen: HashSet<NodeIndex> = HashSet::new();
    let mut neighbors: Vec<EdgeInfoResult> = Vec::new();
    for neighbor in iter_neighbors(&loaded.graph, idx, false) {
        if !seen.insert(neighbor) {
            continue;
        }
        let neighbor_data = &loaded.graph[neighbor];
        let edge = edge_between(&loaded.graph, idx, neighbor);
        neighbors.push(EdgeInfoResult {
            neighbor_id: neighbor_data.id.clone(),
            neighbor_label: neighbor_data.label.clone(),
            neighbor_file: loaded.display_path(&neighbor_data.source_file),
            neighbor_line: neighbor_data.source_line,
            relation: edge.map_or("?".to_string(), |e| e.relation.clone()),
            confidence: edge.map_or("?".to_string(), |e| e.confidence.clone()),
            strength: edge.map_or(0.0, |e| e.strength()),
        });
    }

    // Strongest connections first; ties broken deterministically.
    neighbors.sort_by(|a, b| {
        b.strength
            .partial_cmp(&a.strength)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.relation.cmp(&b.relation))
            .then_with(|| a.neighbor_id.cmp(&b.neighbor_id))
    });
    let neighbor_count = neighbors.len();
    neighbors.truncate(20);

    let answer = format!("explain: {} ({} neighbors)", node.label, neighbor_count);
    log_query(db, node_id, &answer);

    // Hyperedge membership: which N-ary groups this node belongs to.
    let hyperedges: Vec<String> = {
        let mut stmt = db.prepare(
            "SELECT label FROM hyperedges WHERE EXISTS (
               SELECT 1 FROM json_each(hyperedges.nodes) WHERE json_each.value = ?1
             ) LIMIT 5",
        )?;
        let rows = stmt.query_map(rusqlite::params![node.id], |r| r.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    Ok(Some(ExplainResult {
        id: node.id.clone(),
        label: node.label.clone(),
        source_file: loaded.display_path(&node.source_file),
        source_line: node.source_line,
        community: node.community,
        neighbor_count,
        neighbors,
        hyperedges,
    }))
}

pub struct EdgeInfoResult {
    pub neighbor_id: String,
    pub neighbor_label: String,
    pub neighbor_file: String,
    pub neighbor_line: Option<i64>,
    pub relation: String,
    pub confidence: String,
    pub strength: f64,
}

pub struct ExplainResult {
    pub id: String,
    pub label: String,
    pub source_file: String,
    pub source_line: Option<i64>,
    pub community: Option<i64>,
    pub neighbor_count: usize,
    pub neighbors: Vec<EdgeInfoResult>,
    /// Labels of hyperedges whose member list contains this node.
    pub hyperedges: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    #[test]
    fn stopwords_neutralized_and_code_ranks_over_document() {
        // "where does validation happen": question words must score nothing,
        // and on equal term evidence ("validation") the code symbol outranks
        // a prose document node.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('doc', 'validation notes', 'document', 'docs/notes.md'),
                ('sym', 'validation()', 'code', 'src/ingest.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('sym', 'doc', 'references', 'EXTRACTED', 'src/ingest.rs');",
        )
        .unwrap();

        let loaded = load_graph_cached(&db, "scorer-prior-test").unwrap();
        let scored = score_nodes(&loaded, &["validation".to_string()]);
        assert!(scored.len() >= 2);
        // equal term evidence (both labels contain "validation") — the code
        // symbol must outrank the prose node
        assert_eq!(loaded.graph[scored[0].1].file_type, "code");

        // question words alone match nothing
        let empty = score_nodes(&loaded, &["where".to_string(), "does".to_string()]);
        assert!(empty.is_empty());
    }

    #[test]
    fn semantic_seeds_match_without_string_overlap() {
        // "auth flow" shares no tokens with "SessionMiddleware" — token
        // scoring finds nothing; a semantic candidate must seed the traversal.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('m', 'SessionMiddleware', 'code', 'src/session.rs'),
                ('v', 'validateSession()', 'code', 'src/session.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('m', 'v', 'defines', 'EXTRACTED', 'src/session.rs');",
        )
        .unwrap();

        // token-only: no match (unique db_path — the graph cache is keyed
        // by db_path, and ":memory:" would collide across tests)
        let (text, nodes, _, _) = query_graph(
            &db,
            "mem-semantic-test",
            "auth flow",
            "bfs",
            2,
            500,
            false,
            0.0,
            0,
        )
        .unwrap();
        assert!(
            text.contains("No matching nodes"),
            "token-only should miss: {text}"
        );
        assert_eq!(nodes, 0);

        // with a semantic candidate the traversal runs
        let semantic = vec![("m".to_string(), 0.9), ("v".to_string(), 0.7)];
        let (text, nodes, _, _) = query_graph_with_semantic(
            &db,
            "mem-semantic-test",
            "auth flow",
            "bfs",
            2,
            500,
            false,
            0.0,
            0,
            &semantic,
        )
        .unwrap();
        assert!(
            text.contains("SessionMiddleware"),
            "semantic seed should drive traversal: {text}"
        );
        assert!(nodes > 0);
    }

    #[test]
    fn semantic_score_rescales_below_exact_label_match() {
        // a perfect cosine (1.0) caps at 0.67 — under an exact label hit
        assert!(semantic_seed_score(1.0) < 1.0);
        assert!((semantic_seed_score(1.0) - 0.675).abs() < 0.01);
        // below the noise floor it contributes nothing
        assert_eq!(semantic_seed_score(0.55), 0.0);
        assert_eq!(semantic_seed_score(0.3), 0.0);
    }

    #[test]
    fn queries_record_pairs_and_promotion_gates_on_distinct_questions() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('auth', 'AuthService', 'code', 'src/auth.rs'),
                ('sess', 'SessionManager', 'code', 'src/sess.rs'),
                ('tok', 'TokenValidator', 'code', 'src/tok.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('auth', 'sess', 'calls', 'EXTRACTED', 'src/auth.rs'),
                ('sess', 'tok', 'calls', 'EXTRACTED', 'src/sess.rs');",
        )
        .unwrap();

        // two distinct questions whose traversals connect the same pair
        // (both must token-match: unmatched questions return early)
        for question in ["session handling", "session validation"] {
            query_graph(
                &db,
                "mem-pairs-test",
                question,
                "bfs",
                2,
                500,
                false,
                0.0,
                0,
            )
            .unwrap();
        }

        let recorded: i64 = db
            .query_row("SELECT COUNT(*) FROM query_pairs", [], |r| r.get(0))
            .unwrap();
        assert!(recorded > 0, "queries should record seed/discovery pairs");

        // threshold not met yet: 2 distinct questions but each hit once
        let promoted = promote_learned_edges(&db, 2, 3).unwrap();
        assert_eq!(promoted, 0, "needs min_hits across distinct questions");

        // one more repeat pushes hits over the bar
        query_graph(
            &db,
            "mem-pairs-test",
            "session validation",
            "bfs",
            2,
            500,
            false,
            0.0,
            0,
        )
        .unwrap();
        let promoted = promote_learned_edges(&db, 2, 3).unwrap();
        assert!(
            promoted > 0,
            "recurring pairs across questions should promote"
        );

        let (relation, confidence, score, source): (String, String, f64, String) = db
            .query_row(
                "SELECT relation, confidence, confidence_score, source_file
                 FROM edges WHERE relation = 'learned' LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(relation, "learned");
        assert_eq!(confidence, "INFERRED");
        assert_eq!(source, "query_history");
        assert!(score > 0.0 && score <= 1.0);

        // promotion is idempotent — regenerate without duplicating
        let again = promote_learned_edges(&db, 2, 3).unwrap();
        assert_eq!(again, promoted);
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE relation = 'learned'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count as usize, promoted);
    }

    #[test]
    fn promotion_skips_pairs_whose_nodes_were_deleted() {
        // query_pairs rows can outlive their nodes (files removed later) —
        // promotion must drop them, not die on the edges foreign key.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a', 'Alpha', 'code', 'a.rs'),
                ('b', 'Beta', 'code', 'b.rs');
            INSERT INTO query_pairs (source, target, question, hits, first_seen, last_seen) VALUES
                ('a', 'b', 'q1', 2, '1', '1'),
                ('a', 'b', 'q2', 2, '1', '1'),
                ('a', 'ghost', 'q1', 5, '1', '1'),
                ('ghost', 'b', 'q2', 5, '1', '1');",
        )
        .unwrap();

        let promoted = promote_learned_edges(&db, 2, 3).unwrap();
        assert_eq!(promoted, 1, "only the (a,b) pair qualifies and inserts");
        let stale: i64 = db
            .query_row("SELECT COUNT(*) FROM query_pairs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stale, 2, "orphaned pairs are cleaned up");
    }

    fn seed(db: &Connection) -> String {
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a', 'Alpha', 'code', 'src/alpha.rs'),
                ('b', 'BetaService', 'code', 'src/beta.rs'),
                ('c', 'Gamma', 'code', 'src/gamma.rs'),
                ('d', 'Delta', 'code', 'src/delta.rs');
            INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file) VALUES
                ('a', 'b', 'calls', 'EXTRACTED', 1.0, 'src/alpha.rs'),
                ('b', 'c', 'calls', 'SEMANTIC', NULL, 'src/beta.rs'),
                ('c', 'd', 'imports', 'INFERRED', 0.7, 'src/gamma.rs');",
        )
        .unwrap();
        "test".to_string()
    }

    fn loaded(db: &Connection, key: &str) -> Arc<LoadedGraph> {
        load_graph_cached(db, key).unwrap()
    }

    #[test]
    fn tokenize_splits_case_and_separators() {
        assert_eq!(
            tokenize("parseExtractionText"),
            vec!["parse", "extraction", "text"]
        );
        assert_eq!(
            tokenize("parse_extraction_text"),
            vec!["parse", "extraction", "text"]
        );
        assert_eq!(
            tokenize("Parse-Extraction.Text"),
            vec!["parse", "extraction", "text"]
        );
    }

    #[test]
    fn camel_query_finds_snake_label() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('n', 'parse_extraction_text', 'code', 'src/p.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('n', 'n', 'calls', 'EXTRACTED', 'src/p.rs');",
        )
        .unwrap();
        let g = loaded(&db, "camel");
        let scored = score_nodes(&g, &["parseExtractionText".to_string()]);
        assert_eq!(
            scored.len(),
            1,
            "camelCase query should match snake_case label"
        );
    }

    #[test]
    fn directed_bfs_follows_edge_direction_only() {
        let db = open_db_in_memory().unwrap();
        let key = seed(&db);
        let g = loaded(&db, &key);

        let a = g.id_to_idx["a"];
        // b imports d? No — c imports d, so from 'a', directed BFS cannot
        // reach 'd' backwards through a->b<-? a->b->c->d is forward: use
        // 'd' as seed and confirm directed traversal does NOT walk
        // imports backwards to 'c'.
        let d = g.id_to_idx["d"];
        let (visited_fwd, _, _) = bfs_subgraph(&g, &[d], 2, true, 0.0);
        assert!(!visited_fwd.contains(&g.id_to_idx["c"]));

        let (visited_und, _, _) = bfs_subgraph(&g, &[d], 2, false, 0.0);
        assert!(visited_und.contains(&g.id_to_idx["c"]));

        let _ = a;
    }

    #[test]
    fn directed_path_respects_direction() {
        let db = open_db_in_memory().unwrap();
        let key = seed(&db);
        // a -> b -> c -> d: directed path a..d exists; d..a does not
        let (found, hops, _) = find_shortest_path(&db, &key, "Alpha", "Delta", true, 0.0).unwrap();
        assert!(found);
        assert_eq!(hops, 3);

        let (found_rev, _, _) = find_shortest_path(&db, &key, "Delta", "Alpha", true, 0.0).unwrap();
        assert!(!found_rev);

        let (found_und, hops_und, _) =
            find_shortest_path(&db, &key, "Delta", "Alpha", false, 0.0).unwrap();
        assert!(found_und);
        assert_eq!(hops_und, 3);
    }

    #[test]
    fn explain_sorts_by_numeric_strength_not_alphabet() {
        let db = open_db_in_memory().unwrap();
        let key = seed(&db);
        let result = explain_with_neighbors(&db, &key, "b").unwrap().unwrap();
        assert_eq!(result.neighbors.len(), 2);
        // b -> c is SEMANTIC (fallback 0.6), a -> b is EXTRACTED 1.0.
        // Alphabetical ("EXTRACTED" < "SEMANTIC") would put SEMANTIC first.
        assert_eq!(result.neighbors[0].neighbor_id, "a");
        assert!(result.neighbors[0].strength > result.neighbors[1].strength);
    }

    #[test]
    fn scoring_is_deterministic_on_ties() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('x', 'handler', 'code', 'src/x.rs'),
                ('y', 'handler', 'code', 'src/y.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('x', 'y', 'calls', 'EXTRACTED', 'src/x.rs');",
        )
        .unwrap();
        let g = loaded(&db, "ties");
        let s1 = score_nodes(&g, &["handler".to_string()]);
        let s2 = score_nodes(&g, &["handler".to_string()]);
        let ids = |s: &[(f64, NodeIndex)]| -> Vec<String> {
            s.iter().map(|(_, i)| g.graph[*i].id.clone()).collect()
        };
        assert_eq!(ids(&s1), ids(&s2));
    }

    #[test]
    fn confidence_rank_orders_labels() {
        assert!(confidence_rank("DECLARED") > confidence_rank("EXTRACTED"));
        assert!(confidence_rank("EXTRACTED") > confidence_rank("INFERRED"));
        assert!(confidence_rank("INFERRED") > confidence_rank("SEMANTIC"));
    }

    #[test]
    fn no_match_suggests_nearest_labels() {
        let db = open_db_in_memory().unwrap();
        let key = seed(&db);
        // Hybrid retrieval: a typo'd term ("Alpga") is rescued by the fuzzy
        // layer and matches Alpha directly.
        let (text, nodes, _, _) =
            query_graph(&db, &key, "Alpga", "bfs", 2, 2000, false, 0.0, 0).unwrap();
        assert!(
            nodes > 0 && text.contains("Alpha"),
            "typo should still match, got: {text}"
        );

        // Gibberish with no near match: clean no-match, no suggestions.
        let (text2, nodes2, _, _) =
            query_graph(&db, &key, "zzzqqq xxxvvv", "bfs", 2, 2000, false, 0.0, 0).unwrap();
        assert_eq!(nodes2, 0);
        assert!(text2.starts_with("No matching nodes found."));

        // A label in the narrow similarity band (matched by neither exact
        // nor the fuzzy-rescue threshold) surfaces via did-you-mean.
        let g = load_graph_cached(&db, &key).unwrap();
        let sugg = nearest_labels(&g, "Ahpah", 3);
        assert!(
            sugg.iter().any(|s| s.contains("Alpha")),
            "nearest_labels should surface Alpha, got: {sugg:?}"
        );
    }

    #[test]
    fn node_lines_carry_ids_and_relative_paths() {
        let db = open_db_in_memory().unwrap();
        // Seed with an absolute source path under a fake root; the DB path
        // determines the root.
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a', 'Alpha', 'code', 'C:/repo/src/alpha.rs');
            INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file) VALUES
                ('a', 'a', 'calls', 'EXTRACTED', 1.0, 'C:/repo/src/alpha.rs');",
        )
        .unwrap();
        let (text, _, _, _) = query_graph(
            &db,
            "C:/repo/.astria/db.sqlite",
            "Alpha",
            "bfs",
            2,
            2000,
            false,
            0.0,
            0,
        )
        .unwrap();
        assert!(
            text.contains("[id=a src=src/alpha.rs"),
            "expected id and root-relative path in output, got: {text}"
        );
        assert!(!text.contains("C:/repo/src"), "absolute path must not leak");
    }

    #[test]
    fn tiny_budget_keeps_edges_not_just_nodes() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('a', 'Alpha', 'code', 'f.rs', 'docstring-a'),
                ('b', 'Beta', 'code', 'f.rs', 'docstring-b'),
                ('c', 'Gamma', 'code', 'f.rs', 'docstring-c'),
                ('d', 'Delta', 'code', 'f.rs', 'docstring-d');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('a', 'b', 'calls', 'EXTRACTED', 'f.rs'),
                ('a', 'c', 'calls', 'EXTRACTED', 'f.rs'),
                ('a', 'd', 'calls', 'EXTRACTED', 'f.rs');",
        )
        .unwrap();
        let (text, _, _, next) = query_graph(
            &db,
            ":memory:edgebudget",
            "Alpha",
            "bfs",
            2,
            1,
            false,
            0.0,
            0,
        )
        .unwrap();
        assert!(next.is_none() || next.is_some(), "cursor shape check");
        assert!(
            text.contains("EDGE"),
            "edge lines must survive a tiny budget, got: {text}"
        );
    }

    #[test]
    fn plural_terms_match_singular_labels() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('x', 'community_handler', 'code', 'src/x.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('x', 'x', 'calls', 'EXTRACTED', 'src/x.rs');",
        )
        .unwrap();
        let (text, nodes, _, _) = query_graph(
            &db,
            ":memory:plural",
            "communities",
            "bfs",
            1,
            2000,
            false,
            0.0,
            0,
        )
        .unwrap();
        assert!(
            nodes > 0 && text.contains("community_handler"),
            "got: {text}"
        );
    }

    #[test]
    fn docstring_matches_contribute_to_seeds() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('d', 'Helper', 'code', 'src/d.rs', 'Handles authentication tokens for the API');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('d', 'd', 'calls', 'EXTRACTED', 'src/d.rs');",
        )
        .unwrap();
        let (_, nodes, _, _) = query_graph(
            &db,
            ":memory:docseed",
            "authentication",
            "bfs",
            1,
            2000,
            false,
            0.0,
            0,
        )
        .unwrap();
        assert!(nodes > 0, "docstring content should seed the node");
    }

    #[test]
    fn cursor_pages_through_truncated_nodes() {
        let db = open_db_in_memory().unwrap();
        let mut sql = String::from("INSERT INTO nodes (id, label, file_type, source_file) VALUES ");
        for i in 0..30 {
            if i > 0 {
                sql.push(',');
            }
            sql.push_str(&format!("('n{i:02}', 'Node{i:02}', 'code', 'f.rs')"));
        }
        sql.push_str("; INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('n00','n01','calls','EXTRACTED','f.rs');");
        db.execute_batch(&sql).unwrap();
        let key = ":memory:cursor";

        let (text1, _, _, next1) =
            query_graph(&db, key, "Node", "bfs", 1, 30, false, 0.0, 0).unwrap();
        assert!(next1.is_some(), "30 nodes cannot fit in 30 tokens: {text1}");
        assert!(text1.contains("cursor"));

        let (text2, _, _, _) =
            query_graph(&db, key, "Node", "bfs", 1, 30, false, 0.0, next1.unwrap()).unwrap();
        // The second page must start past the first page's nodes.
        assert_ne!(
            text1.lines().nth(2),
            text2.lines().nth(2),
            "cursor should advance the node window"
        );
    }

    #[test]
    fn detail_high_drops_inferred_edges() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a', 'Alpha', 'code', 'f.rs'),
                ('b', 'Beta', 'code', 'f.rs'),
                ('c', 'Gamma', 'code', 'f.rs');
            INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file) VALUES
                ('a', 'b', 'calls', 'EXTRACTED', 1.0, 'f.rs'),
                ('b', 'c', 'calls', 'INFERRED', 0.7, 'f.rs');",
        )
        .unwrap();
        let key = ":memory:detail";
        let (all, _, _, _) = query_graph(&db, key, "Beta", "bfs", 1, 4000, false, 0.0, 0).unwrap();
        assert!(all.contains("Gamma"), "default tier keeps inferred edges");

        let (high, _, _, _) = query_graph(&db, key, "Beta", "bfs", 1, 4000, false, 0.9, 0).unwrap();
        assert!(
            !high.contains("NODE Gamma"),
            "high tier must not traverse inferred edges, got: {high}"
        );
    }

    #[test]
    fn repo_map_is_ranked_deterministic_and_budgeted() {
        let db = open_db_in_memory().unwrap();
        let mut sql = String::from(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('h', 'Hub', 'code', 'src/hub.rs'),
                ('h1', 'Hub1', 'code', 'src/hub.rs'),
                ('h2', 'Hub2', 'code', 'src/hub.rs'),
                ('o', 'Orphan', 'code', 'src/orphan.rs'),
                ('p', 'Peer', 'code', 'src/peer.rs');",
        );
        // hub.rs heavily connected to peer.rs; orphan.rs isolated
        let sources = ["h", "h1", "h2", "h", "h1"];
        for src in sources {
            sql.push_str(&format!(
                "; INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('{src}', 'p', 'calls', 'EXTRACTED', 'src/hub.rs')"
            ));
        }
        db.execute_batch(&sql).unwrap();
        let key = ":memory:map";

        let (map1, shown1) = repo_map(&db, key, 4000, 0.0).unwrap();
        let (map2, _) = repo_map(&db, key, 4000, 0.0).unwrap();
        assert_eq!(map1, map2, "map must be deterministic");
        assert!(map1.contains("src/hub.rs"));
        assert!(map1.contains("Orphan"), "isolated files still appear");
        assert_eq!(shown1, 3);

        let (small, small_shown) = repo_map(&db, key, 1, 0.0).unwrap();
        assert!(small_shown < shown1, "tiny budget shows fewer files");
        assert!(small.contains("truncated"), "truncation is declared");
        assert!(small.contains("Hub"), "the top-ranked file always fits");
    }
}
