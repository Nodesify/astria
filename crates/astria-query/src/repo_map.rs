//! Aider-style budgeted repo orientation map.
use super::*;

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
    let token_budget = usize::try_from(budget)
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| {
            astria_core::AstriaError::Graph(
                "budget must be a positive o200k_base token count".into(),
            )
        })?;
    let empty_response = |message: &str| {
        if count_response_tokens(message) > token_budget {
            Err(astria_core::AstriaError::Graph(
                "budget is too small for a repo-map response".into(),
            ))
        } else {
            Ok((message.to_string(), 0))
        }
    };
    let loaded = load_graph_snapshot(db, db_path)?;
    if loaded.graph.node_count() == 0 {
        return empty_response("No nodes in graph.");
    }

    // Display-form file of each node (indexed by NodeIndex::index()).
    let file_of: Vec<String> = loaded
        .graph
        .node_indices()
        .map(|idx| loaded.display_path(&loaded.graph[idx].source_file))
        .collect();
    let eligible = |idx: NodeIndex| {
        !file_of[idx.index()].trim().is_empty()
            && !matches!(loaded.graph[idx].file_type.as_str(), "stub" | "reference")
    };
    let mut files: Vec<String> = loaded
        .graph
        .node_indices()
        .filter(|&idx| eligible(idx))
        .map(|idx| file_of[idx.index()].clone())
        .collect();
    files.sort();
    files.dedup();
    let n = files.len();
    if n == 0 {
        return empty_response("No located source files in graph.");
    }
    let file_rank: HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.as_str(), i))
        .collect();

    // File-level adjacency: undirected weight = cross-file edge count.
    let mut adj: Vec<HashMap<usize, f64>> = vec![HashMap::new(); n];
    let mut out_sum: Vec<f64> = vec![0.0; n];
    for e in loaded.graph.edge_references() {
        if !eligible(e.source()) || !eligible(e.target()) || !e.weight().meets_detail(min_strength)
        {
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
        if !eligible(idx) {
            continue;
        }
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
        let footer = if shown + 1 < n {
            format!(
                "\n... (map truncated: {} of {} files; raise --budget for more)\n",
                shown + 1,
                n
            )
        } else {
            String::new()
        };
        if count_response_tokens(&format!("{out}{block}{footer}")) > token_budget {
            let footer = format!(
                "\n... (map truncated: {} of {} files; raise --budget for more)\n",
                shown, n
            );
            if count_response_tokens(&format!("{out}{footer}")) <= token_budget {
                out.push_str(&footer);
            }
            break;
        }
        out.push_str(&block);
        shown += 1;
    }

    if count_response_tokens(&out) > token_budget || shown == 0 {
        return Err(astria_core::AstriaError::Graph(
            "budget is too small for a complete repo-map record".into(),
        ));
    }

    Ok((out, shown))
}
