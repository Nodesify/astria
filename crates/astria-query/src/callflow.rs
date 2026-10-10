//! Mermaid call-flow diagram generation from `calls` edges.
use super::*;

/// Mermaid call-flow diagram: breadth-first over `calls` edges from one
/// seed node. Direction "out" renders what the node calls, "in" renders
/// what calls it, "both" renders the union. Output is a `flowchart LR`
/// block - GitHub, Obsidian, and mermaid.js render it natively.
pub fn callflow_mermaid(
    db: &Connection,
    db_path: &str,
    seed_query: &str,
    depth: usize,
    direction: &str,
) -> astria_core::Result<String> {
    let _transaction = read_snapshot(db)?;
    let loaded = load_graph(db, db_path)?;
    if loaded.graph.node_count() == 0 {
        return Ok("No nodes in graph.".to_string());
    }

    // Seed resolution mirrors affected/explain: exact id wins, a stub never
    // shadows a same-named definition, fuzzy scoring as the last resort.
    let resolve = |q: &str| -> Option<NodeIndex> {
        if let Some(&idx) = loaded.id_to_idx.get(q) {
            let is_stub: bool = db
                .query_row(
                    "SELECT file_type = 'stub' FROM nodes WHERE id = ?1",
                    rusqlite::params![q],
                    |r| r.get(0),
                )
                .unwrap_or(false);
            if !is_stub {
                return Some(idx);
            }
            let bare = q
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
        let terms: Vec<String> = q.split_whitespace().map(|s| s.to_string()).collect();
        score_nodes(&loaded, &terms).ranked.first().map(|(_, i)| *i)
    };

    let seed = resolve(seed_query).ok_or_else(|| {
        astria_core::AstriaError::Graph(format!("node not found: '{seed_query}'"))
    })?;

    let outgoing = direction != "in";
    let incoming = direction != "out";

    let mut seen: HashSet<NodeIndex> = HashSet::new();
    seen.insert(seed);
    let mut frontier: Vec<NodeIndex> = vec![seed];
    let mut distance: HashMap<NodeIndex, u32> = [(seed, 0)].into_iter().collect();
    let mut call_edges: Vec<(NodeIndex, NodeIndex)> = Vec::new();

    for level in 0..depth {
        let mut next: Vec<NodeIndex> = Vec::new();
        for &n in &frontier {
            let mut consider = |other: NodeIndex| {
                if !seen.contains(&other) {
                    seen.insert(other);
                    distance.insert(other, level as u32 + 1);
                    next.push(other);
                }
            };
            if outgoing {
                for e in loaded
                    .graph
                    .edges_directed(n, petgraph::Direction::Outgoing)
                {
                    if e.weight().relation == "calls" {
                        let t = e.target();
                        call_edges.push((n, t));
                        consider(t);
                    }
                }
            }
            if incoming {
                for e in loaded
                    .graph
                    .edges_directed(n, petgraph::Direction::Incoming)
                {
                    if e.weight().relation == "calls" {
                        let src = e.source();
                        call_edges.push((src, n));
                        consider(src);
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }

    // Stable render order: hop distance from the seed, then label, then id.
    let mut nodes: Vec<(u32, String, String)> = seen
        .iter()
        .map(|idx| {
            (
                distance.get(idx).copied().unwrap_or(u32::MAX),
                loaded.graph[*idx].label.clone(),
                loaded.graph[*idx].id.clone(),
            )
        })
        .collect();
    nodes.sort();

    let mermaid_id: HashMap<String, String> = nodes
        .iter()
        .enumerate()
        .map(|(i, (_, _, id))| (id.clone(), format!("n{i}")))
        .collect();

    let mut out = String::from(
        "flowchart LR
",
    );
    for (_, label, id) in &nodes {
        let short: String = label.chars().take(60).collect();
        let escaped = short.replace('"', "'");
        out.push_str(&format!(
            "  {}[\"{}\"]
",
            mermaid_id[id], escaped
        ));
    }

    let mut rendered: Vec<(String, String)> = call_edges
        .iter()
        .filter_map(|(a, b)| {
            let sa = loaded.graph[*a].id.clone();
            let sb = loaded.graph[*b].id.clone();
            let ma = mermaid_id.get(&sa)?;
            let mb = mermaid_id.get(&sb)?;
            Some((ma.clone(), mb.clone()))
        })
        .collect();
    rendered.sort();
    rendered.dedup();
    for (a, b) in &rendered {
        out.push_str(&format!(
            "  {a} --> {b}
"
        ));
    }
    Ok(out)
}
