//! `explain`: one node with its neighbors, hyperedges and evidence.
use super::*;

pub fn explain_with_neighbors(
    db: &Connection,
    db_path: &str,
    node_id: &str,
) -> astria_core::Result<Option<ExplainResult>> {
    let transaction = read_snapshot(db)?;
    let loaded = load_graph(db, db_path)?;

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
            match scored.ranked.first() {
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
        // The stored orientation says which way the edge points: outgoing
        // (this node → neighbor, e.g. it calls the neighbor) or incoming
        // (neighbor → this node, e.g. the neighbor calls it). Rendering
        // every connection as if the explained node were the source
        // inverts caller/callee and misleads agents reading it.
        let forward = loaded
            .graph
            .edges_directed(idx, Direction::Outgoing)
            .find(|e| e.target() == neighbor)
            .map(|e| e.weight());
        let outgoing = forward.is_some();
        let edge = forward.or_else(|| edge_between(&loaded.graph, idx, neighbor));
        neighbors.push(EdgeInfoResult {
            neighbor_id: neighbor_data.id.clone(),
            neighbor_label: neighbor_data.label.clone(),
            neighbor_file: loaded.display_path(&neighbor_data.source_file),
            neighbor_line: neighbor_data.source_line,
            outgoing,
            relation: edge.map_or("?".to_string(), |e| e.relation.clone()),
            confidence: edge.map_or("?".to_string(), |e| e.confidence.clone()),
            strength: edge.map_or(0.0, |e| e.strength()),
            confidence_score: edge.and_then(|e| e.confidence_score),
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

    if let Some(transaction) = transaction {
        transaction.commit()?;
    }
    log_query(db, node_id, &answer);

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
    /// True when the stored edge points from the explained node to this
    /// neighbor (it calls/imports the neighbor); false when the neighbor
    /// points back (the neighbor calls/imports the explained node).
    pub outgoing: bool,
    pub relation: String,
    pub confidence: String,
    pub strength: f64,
    /// The stored numeric score when one exists (Jev keep-probability on
    /// verified semantic edges); None means the label rank is all there is.
    pub confidence_score: Option<f64>,
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
