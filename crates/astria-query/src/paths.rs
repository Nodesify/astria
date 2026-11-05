//! Shortest-path queries over the loaded graph.
use super::*;

/// Evidence tag for path rendering: the stored numeric score rides along
/// with the label (`[INFERRED 0.75]`); `EXTRACTED`/`DECLARED` and edges
/// without a usable score keep the bare label.
fn confidence_tag(edge: &EdgeData) -> String {
    let bare = matches!(
        edge.confidence.to_ascii_uppercase().as_str(),
        "EXTRACTED" | "DECLARED"
    );
    match edge.confidence_score {
        Some(score) if score.is_finite() && !bare => format!("[{} {:.2}]", edge.confidence, score),
        _ => format!("[{}]", edge.confidence),
    }
}

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
    let path = match shortest_path_conservative(
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

    let hops = path.edges.len();
    let mut text = format!("Shortest path ({} hops):\n", hops);

    for (i, edge_id) in path.edges.iter().enumerate() {
        let (source, target) = loaded
            .graph
            .edge_endpoints(*edge_id)
            .expect("traversed edge exists");
        let edge = &loaded.graph[*edge_id];
        let marker = if path.weakest_hop == Some(i) {
            " *"
        } else {
            ""
        };
        text.push_str(&format!(
            "  {} --{} {}{}--> {}\n",
            loaded.graph[source].label,
            edge.relation,
            confidence_tag(edge),
            marker,
            loaded.graph[target].label
        ));
        // Weaker parallel evidence for the same hop: preserved, not dropped.
        for extra in &path.also_seen[i] {
            let (xs, xt) = loaded
                .graph
                .edge_endpoints(*extra)
                .expect("parallel edge exists");
            let xedge = &loaded.graph[*extra];
            text.push_str(&format!(
                "    also seen: {} --{} {}--> {}\n",
                loaded.graph[xs].label,
                xedge.relation,
                confidence_tag(xedge),
                loaded.graph[xt].label
            ));
        }
    }

    // The route's overall confidence is its weakest hop — reported by name so
    // the tight spot cannot hide behind stronger neighbors. When ties on
    // length existed, the most conservative of them was chosen.
    if let Some(pos) = path.weakest_hop {
        let (source, target) = loaded
            .graph
            .edge_endpoints(path.edges[pos])
            .expect("traversed edge exists");
        let edge = &loaded.graph[path.edges[pos]];
        let route_note = if path.routes > 1 {
            let count = if path.routes_capped {
                format!(">{}", ROUTE_CAP)
            } else {
                path.routes.to_string()
            };
            format!("; most conservative of {count} equal-length routes")
        } else {
            String::new()
        };
        text.push_str(&format!(
            "Path confidence: {} — weakest hop is {} --{}--> {}{}\n",
            confidence_tag(edge),
            loaded.graph[source].label,
            edge.relation,
            loaded.graph[target].label,
            route_note
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
