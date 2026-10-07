//! Shortest-path queries over the loaded graph.
use super::*;

pub fn find_shortest_path(
    db: &Connection,
    db_path: &str,
    source_query: &str,
    target_query: &str,
    directed: bool,
    min_strength: f64,
) -> astria_core::Result<(bool, usize, String)> {
    let transaction = read_snapshot(db)?;
    let loaded = load_graph(db, db_path)?;
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
            match src_scored.ranked.first() {
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
            match tgt_scored.ranked.first() {
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

    let semantic_floor = std::env::var("ASTRIA_QUERY_MIN_SEMANTIC_CONFIDENCE")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let path = match shortest_path_bfs(
        &loaded,
        src_idx,
        tgt_idx,
        directed,
        min_strength,
        semantic_floor,
    ) {
        Some(p) => p,
        None => return Ok((false, 0, "No path found.".to_string())),
    };

    let hops = path.len();
    let mut text = format!("Shortest path ({} hops):\n", hops);

    for edge_id in path {
        let (source, target) = loaded
            .graph
            .edge_endpoints(edge_id)
            .expect("traversed edge exists");
        let edge = &loaded.graph[edge_id];
        text.push_str(&format!(
            "  {} --{} [{}]--> {}\n",
            loaded.graph[source].label, edge.relation, edge.confidence, loaded.graph[target].label
        ));
    }

    if let Some(transaction) = transaction {
        transaction.commit()?;
    }
    let answer = format!("path found: {} hops", hops);
    log_query(
        db,
        &format!("{} -> {}", source_query, target_query),
        &answer,
    );

    Ok((true, hops, text))
}
