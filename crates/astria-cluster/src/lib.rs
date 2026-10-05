mod naming;

// astria-cluster: label propagation clustering with source module community
// labels and oversized-community
// splitting (ported from upstream astria v8).

use petgraph::graph::NodeIndex;
use petgraph::graph::UnGraph;
use rusqlite::Connection;
use std::collections::HashMap;

/// Communities larger than 25% of the graph (and at least OVERSIZED_MIN
/// nodes) are re-partitioned — one giant community swallows the report.
const OVERSIZED_SHARE: f64 = 0.25;
const OVERSIZED_MIN: usize = 10;

/// Communities smaller than UNDERSIZED_MIN nodes that share at least one
/// edge with another community are merged into their strongest neighbor.
/// Judge-pruned semantic edges and near-duplicate removal strand one- and
/// two-node fragments by the hundreds; they carry no theme and bury every
/// community listing. Truly isolated nodes (no edges at all) keep their
/// own community because there is nothing to merge into.
const UNDERSIZED_MIN: usize = 3;

/// A node is a hub when its degree reaches at least this, scaled up with the
/// graph's mean degree — hubs are what glue communities together, so only
/// `--exclude-hubs` needs them and the bar must sit far above ordinary
/// connector symbols.
const HUB_MIN_DEGREE: usize = 12;

/// Knobs for `cluster_with`. Defaults reproduce the classic label-propagation
/// behavior exactly; `astria cluster-only --resolution/--exclude-hubs` sets
/// them.
#[derive(Debug, Clone)]
pub struct ClusterOptions {
    /// Minimum share of a node's neighbors that must back the winning label
    /// before the node joins it, in [0.0, 1.0]. 0.0 = join the dominant
    /// neighbor label (classic propagation, the default); 1.0 requires
    /// unanimous neighbors. Higher values → more, smaller communities.
    pub resolution: f64,
    /// Keep high-degree hub nodes out of label propagation so they cannot
    /// bridge every community into one. Hubs are attached to their strongest
    /// community after propagation, so every node still gets a community.
    pub exclude_hubs: bool,
}

impl Default for ClusterOptions {
    fn default() -> Self {
        Self {
            resolution: 0.0,
            exclude_hubs: false,
        }
    }
}

#[derive(Debug)]
pub struct ClusterResult {
    pub communities: HashMap<u32, usize>,
    /// Source module/package labels, preserving valid LLM enrichment.
    pub labels: HashMap<u32, String>,
    pub iterations: u32,
    /// Newman modularity of the final partition in [-1, 1].
    pub modularity: f64,
    /// Nodes held out of propagation by `exclude_hubs` (0 when the flag is
    /// off) — they were assigned communities only after the fact.
    pub excluded_hubs: usize,
}

pub fn cluster(db: &Connection) -> astria_core::Result<ClusterResult> {
    cluster_with(db, &ClusterOptions::default())
}

pub fn cluster_with(
    db: &Connection,
    options: &ClusterOptions,
) -> astria_core::Result<ClusterResult> {
    let resolution = options.resolution.clamp(0.0, 1.0);
    // Load source loci for module naming. Ordered by id so label
    // propagation is deterministic across runs and platforms.
    let node_ids: Vec<String> = {
        let mut stmt = db.prepare("SELECT id FROM nodes ORDER BY id")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let node_sources: HashMap<String, (String, String)> = {
        let mut stmt = db.prepare("SELECT id, source_file, file_type FROM nodes")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (row.get::<_, String>(1)?, row.get::<_, String>(2)?),
            ))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };

    if node_ids.is_empty() {
        return Ok(ClusterResult {
            communities: HashMap::new(),
            labels: HashMap::new(),
            iterations: 0,
            modularity: 0.0,
            excluded_hubs: 0,
        });
    }

    let id_to_idx: HashMap<String, NodeIndex> = node_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), NodeIndex::new(i)))
        .collect();

    let mut graph = UnGraph::<String, ()>::new_undirected();
    for id in &node_ids {
        graph.add_node(id.clone());
    }

    // Load edges
    {
        let mut stmt = db.prepare("SELECT source, target FROM edges")?;
        let edges: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (src, tgt) in edges {
            if let (Some(&s), Some(&t)) = (id_to_idx.get(&src), id_to_idx.get(&tgt)) {
                graph.add_edge(s, t, ());
            }
        }
    }

    // Hub exclusion (`--exclude-hubs`): hubs keep their unique initial label
    // out of the tally — no neighbor can adopt a hub's label, and hubs adopt
    // none — then are attached to their strongest community after the merge
    // passes, so every node still ends up in a community.
    let excluded: std::collections::HashSet<usize> = if options.exclude_hubs {
        hub_nodes(&graph)
    } else {
        Default::default()
    };
    let excluded_hubs = excluded.len();

    // Label propagation
    let n = node_ids.len();
    let mut labels: Vec<u32> = (0..n as u32).collect();
    let iterations = propagate(&graph, &mut labels, resolution, Some(&excluded));

    // Split oversized communities by re-running propagation on the subgraph.
    for _ in 0..3 {
        let sizes = sizes_of(&labels);
        let oversized: Vec<u32> = sizes
            .iter()
            .filter(|(_, &size)| {
                size >= OVERSIZED_MIN && (size as f64) > n as f64 * OVERSIZED_SHARE
            })
            .map(|(&label, _)| label)
            .collect();
        if oversized.is_empty() {
            break;
        }
        let mut next_id = labels.iter().copied().max().unwrap_or(0) + 1;
        let mut split_happened = false;
        for big in oversized {
            let members: Vec<usize> = labels
                .iter()
                .enumerate()
                .filter(|(_, &l)| l == big)
                .map(|(i, _)| i)
                .collect();
            let mut sub_labels: Vec<u32> = (0..members.len() as u32).collect();
            let sub_graph = induced_subgraph(&graph, &members);
            // Hub exclusion does not apply inside oversized-community splits:
            // hubs hold unique labels and can never be members of a split
            // candidate, and re-deriving hub thresholds on the subgraph would
            // flag ordinary connectors.
            propagate(&sub_graph, &mut sub_labels, resolution, None);
            let distinct: std::collections::HashSet<u32> = sub_labels.iter().copied().collect();
            if distinct.len() > 1 {
                split_happened = true;
                // Largest piece keeps the original community id for stability
                let mut sub_sizes: HashMap<u32, usize> = HashMap::new();
                for &sl in &sub_labels {
                    *sub_sizes.entry(sl).or_insert(0) += 1;
                }
                let keep = *sub_sizes
                    .iter()
                    .max_by_key(|(_, &s)| s)
                    .map(|(l, _)| l)
                    .unwrap();
                let mut sub_remap: HashMap<u32, u32> = HashMap::new();
                sub_remap.insert(keep, big);
                for (pos, member) in members.iter().enumerate() {
                    let sl = sub_labels[pos];
                    let target = *sub_remap.entry(sl).or_insert_with(|| {
                        let id = next_id;
                        next_id += 1;
                        id
                    });
                    labels[*member] = target;
                }
            }
        }
        if !split_happened {
            break;
        }
    }

    // Merge undersized fragments into their strongest neighbor, always
    // taking the mergeable fragment whose first member has the lowest
    // index so the partition stays deterministic. Each merge strictly
    // reduces the community count, and fragments with no cross edges are
    // recorded as isolated (their edges never change, so they can never
    // become mergeable) — a pass with neither terminates the loop.
    // Excluded hubs sit in singleton communities until the assignment pass
    // below, so they must never be merged as fragments.
    let mut isolated: std::collections::HashSet<u32> =
        excluded.iter().map(|&i| labels[i]).collect();
    loop {
        let sizes = sizes_of(&labels);
        let mut fragment = None;
        for &l in labels.iter() {
            if sizes[&l] < UNDERSIZED_MIN && !isolated.contains(&l) {
                fragment = Some(l);
                break;
            }
        }
        let Some(fragment) = fragment else { break };
        let members: Vec<usize> = labels
            .iter()
            .enumerate()
            .filter(|(_, &l)| l == fragment)
            .map(|(i, _)| i)
            .collect();
        // Tally cross edges from every fragment member to each neighboring
        // community; the most-connected one absorbs the fragment. Ties go
        // to the lower community id. Excluded hubs are skipped — their
        // singleton labels must not absorb fragments; the assignment pass
        // below pulls hubs toward communities, never the reverse.
        let mut tally: HashMap<u32, usize> = HashMap::new();
        for &member in &members {
            for neighbor in graph.neighbors(NodeIndex::new(member)) {
                if excluded.contains(&neighbor.index()) {
                    continue;
                }
                let nl = labels[neighbor.index()];
                if nl != fragment {
                    *tally.entry(nl).or_insert(0) += 1;
                }
            }
        }
        let Some((&target, _)) = tally
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        else {
            isolated.insert(fragment);
            continue;
        };
        for &member in &members {
            labels[member] = target;
        }
    }

    // Attach excluded hubs: each joins the community it shares the most
    // edges with (ties → lower community id, deterministic). Hubs are
    // ordered by index so assignment order cannot change results; hub↔hub
    // edges contribute nothing because neither side has a final community
    // until its own turn — exactly the "strongest *community*" semantics.
    for &hub in &excluded {
        let mut tally: HashMap<u32, usize> = HashMap::new();
        for neighbor in graph.neighbors(NodeIndex::new(hub)) {
            if excluded.contains(&neighbor.index()) {
                continue;
            }
            *tally.entry(labels[neighbor.index()]).or_insert(0) += 1;
        }
        if let Some((&target, _)) = tally
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        {
            labels[hub] = target;
        }
    }

    // Renumber to contiguous ids 0..k-1 for stable reports
    let mut remap: HashMap<u32, u32> = HashMap::new();
    for &l in &labels {
        if !remap.contains_key(&l) {
            let next = remap.len() as u32;
            remap.insert(l, next);
        }
    }
    let labels: Vec<u32> = labels.iter().map(|l| remap[l]).collect();

    // Write communities back to SQLite
    for (i, id) in node_ids.iter().enumerate() {
        db.execute(
            "UPDATE nodes SET community = ?1 WHERE id = ?2",
            rusqlite::params![labels[i] as i64, id],
        )?;
    }

    let mut communities: HashMap<u32, usize> = HashMap::new();
    for &label in &labels {
        *communities.entry(label).or_insert(0) += 1;
    }

    // Community cohesion, persisted for the report and exports
    let mut internal_edges: HashMap<u32, usize> = HashMap::new();
    let mut boundary_edges: HashMap<u32, usize> = HashMap::new();
    let mut cohesion: HashMap<u32, f64> = HashMap::new();

    for edge in graph.edge_indices() {
        let (s, t) = graph.edge_endpoints(edge).unwrap();
        let (cs, ct) = (labels[s.index()], labels[t.index()]);
        if cs == ct {
            *internal_edges.entry(cs).or_insert(0) += 1;
        } else {
            *boundary_edges.entry(cs).or_insert(0) += 1;
            *boundary_edges.entry(ct).or_insert(0) += 1;
        }
    }
    for &c in communities.keys() {
        let internal = internal_edges.get(&c).copied().unwrap_or(0);
        let boundary = boundary_edges.get(&c).copied().unwrap_or(0);
        cohesion.insert(
            c,
            if internal + boundary == 0 {
                0.0
            } else {
                internal as f64 / (internal + boundary) as f64
            },
        );
    }

    // Newman modularity: Q = Σ_c [ internal_c/m − (degree_sum_c / 2m)² ]
    let m = graph.edge_count();
    let modularity = if m == 0 {
        0.0
    } else {
        let mut degree_sum: HashMap<u32, usize> = HashMap::new();
        for (i, _) in node_ids.iter().enumerate() {
            let degree = graph.neighbors(NodeIndex::new(i)).count();
            *degree_sum.entry(labels[i]).or_insert(0) += degree;
        }
        let two_m = 2.0 * m as f64;
        communities
            .keys()
            .map(|&c| {
                let internal = internal_edges.get(&c).copied().unwrap_or(0) as f64;
                let k_c = degree_sum.get(&c).copied().unwrap_or(0) as f64;
                internal / m as f64 - (k_c / two_m).powi(2)
            })
            .sum()
    };

    let final_labels = naming::source_labels(db, &node_ids, &labels, &node_sources);

    // LLM enrichment from the previous build: a community whose membership
    // is unchanged keeps its `--label-communities` name/summary across
    // rebuilds; a changed membership falls back to the fresh source module
    // label (a stale LLM name for a different group would be misleading).
    // Community ids may shift after a rebuild; membership, not the id,
    // determines whether an existing LLM label is still valid.
    let previous_enrichment: HashMap<String, (String, Option<String>)> = {
        let mut stmt = db.prepare(
            "SELECT label, summary, label_source, member_hash FROM communities ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        let mut enrichment = HashMap::new();
        for row in rows {
            let (label, summary, source, hash) = row?;
            if source == "llm" && !label.trim().is_empty() {
                if let Some(hash) = hash.filter(|h| !h.is_empty()) {
                    enrichment.entry(hash).or_insert((label, summary));
                }
            }
        }
        enrichment
    };

    // Membership hash per (new) community id, written alongside the label so
    // the labeling stage can skip unchanged communities without a call.
    let mut member_hashes: HashMap<u32, String> = HashMap::new();
    for &c in communities.keys() {
        let mut ids: Vec<&str> = node_ids
            .iter()
            .enumerate()
            .filter(|(i, _)| labels[*i] == c)
            .map(|(_, id)| id.as_str())
            .collect();
        ids.sort_unstable();
        member_hashes.insert(c, astria_core::db::community_member_hash(&ids));
    }

    db.execute("DELETE FROM communities", [])?;
    {
        let mut stmt = db.prepare(
            "INSERT OR REPLACE INTO communities (id, label, summary, label_source, cohesion, size, member_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for (&c, &size) in &communities {
            let hash = member_hashes.get(&c).map(|s| s.as_str());
            // Carry the previous LLM name over only when the membership is
            // byte-identical; everything else gets the deterministic label.
            let preserved = hash.and_then(|h| previous_enrichment.get(h)).cloned();
            let (label, summary, label_source) = match preserved {
                Some((label, summary)) => (label, summary, "llm".to_string()),
                None => (
                    final_labels
                        .get(&c)
                        .cloned()
                        .unwrap_or_else(|| format!("Community {c}")),
                    None,
                    "source".to_string(),
                ),
            };
            stmt.execute(rusqlite::params![
                c as i64,
                label,
                summary,
                label_source,
                cohesion.get(&c).copied(),
                size as i64,
                hash,
            ])?;
        }
    }
    db.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('last_modularity', ?1)",
        rusqlite::params![format!("{modularity:.6}")],
    )?;

    // The DB is the single source of truth for labels: re-read so the
    // returned map reflects preserved LLM names, not just the fresh
    // source module ones.
    let labels: HashMap<u32, String> = {
        let mut stmt = db.prepare("SELECT id, label FROM communities")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<(u32, String)>>>()?;
        rows.into_iter().collect()
    };

    Ok(ClusterResult {
        communities,
        labels,
        iterations,
        modularity,
        excluded_hubs,
    })
}

/// Nodes whose degree reaches the hub bar: at least HUB_MIN_DEGREE, and at
/// least 4× the graph's mean degree. Both bars must clear, so small graphs
/// (where 12 edges around one node is normal) never gain phantom hubs.
fn hub_nodes(graph: &UnGraph<String, ()>) -> std::collections::HashSet<usize> {
    let n = graph.node_count();
    let mut out = std::collections::HashSet::new();
    if n == 0 {
        return out;
    }
    let degrees: Vec<usize> = (0..n)
        .map(|i| graph.neighbors(NodeIndex::new(i)).count())
        .collect();
    let mean = degrees.iter().sum::<usize>() as f64 / n as f64;
    let threshold = (HUB_MIN_DEGREE as f64).max(4.0 * mean);
    for (i, &d) in degrees.iter().enumerate() {
        if d as f64 >= threshold {
            out.insert(i);
        }
    }
    out
}

/// One full label-propagation pass loop. Returns iterations used.
///
/// `resolution` is the minimum share of a node's (non-excluded) neighbors
/// that must back the winning label before the node joins it: 0.0 joins the
/// dominant label unconditionally (classic propagation), 1.0 requires
/// unanimity. `excluded` nodes neither vote nor update — their labels stay
/// untouched until the caller assigns them.
///
/// Tie-breaking is deterministic: among labels with the maximum neighbor
/// count, the smallest label id wins. Rust's HashMap iteration order is
/// randomized per process, so `max_by_key` alone would make communities
/// (and every downstream report) differ between runs.
fn propagate(
    graph: &UnGraph<String, ()>,
    labels: &mut [u32],
    resolution: f64,
    excluded: Option<&std::collections::HashSet<usize>>,
) -> u32 {
    let is_excluded = |i: usize| excluded.is_some_and(|e| e.contains(&i));
    let n = labels.len();
    let mut iterations = 0;
    for _ in 0..100 {
        iterations += 1;
        let mut changed = false;
        for i in 0..n {
            if is_excluded(i) {
                continue;
            }
            let node_idx = NodeIndex::new(i);
            let mut neighbor_labels: HashMap<u32, usize> = HashMap::new();
            let mut neighbor_total = 0usize;
            for neighbor in graph.neighbors(node_idx) {
                if is_excluded(neighbor.index()) {
                    continue;
                }
                *neighbor_labels.entry(labels[neighbor.index()]).or_insert(0) += 1;
                neighbor_total += 1;
            }
            if neighbor_total == 0 {
                continue;
            }
            let max_count = neighbor_labels.values().copied().max().unwrap_or(0);
            // Smallest label id among the maxima: fully deterministic
            // (HashMap iteration order must not decide communities).
            let best_label = neighbor_labels
                .iter()
                .filter(|(_, &count)| count == max_count)
                .map(|(&label, _)| label)
                .min()
                .unwrap_or(labels[i]);
            if best_label == labels[i] {
                continue;
            }
            if (max_count as f64) >= resolution * neighbor_total as f64 {
                labels[i] = best_label;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    iterations
}

fn sizes_of(labels: &[u32]) -> HashMap<u32, usize> {
    let mut sizes: HashMap<u32, usize> = HashMap::new();
    for &l in labels {
        *sizes.entry(l).or_insert(0) += 1;
    }
    sizes
}

/// Build the induced subgraph on `members` (indices into the original graph).
fn induced_subgraph(graph: &UnGraph<String, ()>, members: &[usize]) -> UnGraph<String, ()> {
    let mut sub = UnGraph::<String, ()>::new_undirected();
    let mut local: HashMap<usize, NodeIndex> = HashMap::new();
    for &m in members {
        local.insert(m, sub.add_node(String::new()));
    }
    for &m in members {
        if let Some(&lm) = local.get(&m) {
            for neighbor in graph.neighbors(NodeIndex::new(m)) {
                if let Some(&ln) = local.get(&neighbor.index()) {
                    if lm < ln {
                        sub.add_edge(lm, ln, ());
                    }
                }
            }
        }
    }
    sub
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    fn seed_graph(db: &Connection) {
        db.execute_batch("
            INSERT INTO nodes (id, label, file_type, source_file) VALUES ('a', 'A', 'code', 'f.py');
            INSERT INTO nodes (id, label, file_type, source_file) VALUES ('b', 'B', 'code', 'f.py');
            INSERT INTO nodes (id, label, file_type, source_file) VALUES ('c', 'C', 'code', 'f.py');
            INSERT INTO nodes (id, label, file_type, source_file) VALUES ('d', 'D', 'code', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('a', 'b', 'calls', 'EXTRACTED', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('b', 'c', 'calls', 'EXTRACTED', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('c', 'd', 'calls', 'EXTRACTED', 'f.py');
        ").unwrap();
    }

    #[test]
    fn cluster_assigns_communities() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);
        let result = cluster(&db).unwrap();
        assert!(!result.communities.is_empty());
        assert!(result.iterations > 0);
        let community: i64 = db
            .query_row("SELECT community FROM nodes WHERE id = 'a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(community >= 0);
    }

    #[test]
    fn connected_graph_few_communities() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);
        let result = cluster(&db).unwrap();
        // Label propagation on a chain can produce 1-3 communities depending on iteration order
        assert!(
            result.communities.len() <= 4,
            "expected at most 4 communities (one per node), got {}",
            result.communities.len()
        );
        assert!(!result.communities.is_empty());
    }

    #[test]
    fn empty_graph_no_crash() {
        let db = open_db_in_memory().unwrap();
        let result = cluster(&db).unwrap();
        assert_eq!(result.communities.len(), 0);
    }

    #[test]
    fn undersized_fragment_merges_into_strongest_neighbor_and_isolated_node_stays() {
        // A clique of four plus a two-node fragment attached to 'c', plus a
        // truly isolated node. The fragment (size 2 < UNDERSIZED_MIN) must
        // merge into the clique's community; the isolated node has no edges
        // and keeps its own community.
        let db = open_db_in_memory().unwrap();
        db.execute_batch("
            INSERT INTO nodes (id, label, file_type, source_file) VALUES ('a', 'A', 'code', 'f.py');
            INSERT INTO nodes (id, label, file_type, source_file) VALUES ('b', 'B', 'code', 'f.py');
            INSERT INTO nodes (id, label, file_type, source_file) VALUES ('c', 'C', 'code', 'f.py');
            INSERT INTO nodes (id, label, file_type, source_file) VALUES ('d', 'D', 'code', 'f.py');
            INSERT INTO nodes (id, label, file_type, source_file) VALUES ('f1', 'F1', 'code', 'g.py');
            INSERT INTO nodes (id, label, file_type, source_file) VALUES ('f2', 'F2', 'code', 'g.py');
            INSERT INTO nodes (id, label, file_type, source_file) VALUES ('solo', 'Solo', 'code', 'h.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('a', 'b', 'calls', 'EXTRACTED', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('a', 'c', 'calls', 'EXTRACTED', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('a', 'd', 'calls', 'EXTRACTED', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('b', 'c', 'calls', 'EXTRACTED', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('b', 'd', 'calls', 'EXTRACTED', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('c', 'd', 'calls', 'EXTRACTED', 'f.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('c', 'f1', 'calls', 'EXTRACTED', 'g.py');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('f1', 'f2', 'calls', 'EXTRACTED', 'g.py');
        ").unwrap();
        let result = cluster(&db).unwrap();
        let community: HashMap<String, i64> = {
            let mut stmt = db.prepare("SELECT id, community FROM nodes").unwrap();
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        // Fragment joined the clique: f1/f2 share their community with c.
        assert_eq!(community["f1"], community["c"]);
        assert_eq!(community["f2"], community["f1"]);
        // Isolated node kept its own community.
        assert_ne!(community["solo"], community["c"]);
        // The clique itself stayed one community.
        assert_eq!(community["a"], community["d"]);
        // Exactly two communities remain: the merged clique+fragment and solo.
        assert_eq!(result.communities.len(), 2, "{:?}", result.communities);
    }

    #[test]
    fn oversized_community_is_split() {
        let db = open_db_in_memory().unwrap();
        // Two dense clusters of 8 nodes each, cross-linked by a single edge.
        // 16 nodes with a giant blob would trigger the split if they merged.
        let mut sql = String::new();
        for i in 0..16 {
            sql.push_str(&format!(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('n{i}', 'N{i}', 'code', 'f.py');\n"
            ));
        }
        for i in 0..8 {
            for j in (i + 1)..8 {
                sql.push_str(&format!(
                    "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('n{i}', 'n{j}', 'calls', 'EXTRACTED', 'f.py');\n"
                ));
                sql.push_str(&format!(
                    "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('n{}', 'n{}', 'calls', 'EXTRACTED', 'f.py');\n",
                    i + 8, j + 8
                ));
            }
        }
        // single weak cross-link between the two clusters
        sql.push_str("INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('n7', 'n8', 'calls', 'EXTRACTED', 'f.py');\n");
        db.execute_batch(&sql).unwrap();
        let result = cluster(&db).unwrap();
        // No single community holds everything
        let max_size = result.communities.values().copied().max().unwrap_or(0);
        assert!(
            max_size < result.communities.values().sum::<usize>(),
            "oversized community should have been split"
        );
    }

    #[test]
    fn clustering_is_deterministic_across_runs() {
        // Two identically-seeded databases must produce identical
        // assignments — label propagation must not depend on HashMap
        // iteration order.
        let assignments = |seed: &dyn Fn(&Connection)| {
            let db = open_db_in_memory().unwrap();
            seed(&db);
            cluster(&db).unwrap();
            let mut rows: Vec<(String, i64)> = db
                .prepare("SELECT id, community FROM nodes ORDER BY id")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();
            rows.sort();
            rows
        };
        let seed = |db: &Connection| {
            // Chain + star + isolated node: plenty of label-count ties
            db.execute_batch(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                    ('a','A','code','f.py'),('b','B','code','f.py'),('c','C','code','f.py'),
                    ('d','D','code','f.py'),('e','E','code','f.py'),('f','F','code','f.py'),
                    ('g','G','code','f.py'),('h','H','code','f.py');
                 INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                    ('a','b','calls','EXTRACTED','f.py'),('b','c','calls','EXTRACTED','f.py'),
                    ('a','c','calls','EXTRACTED','f.py'),('d','e','calls','EXTRACTED','f.py'),
                    ('e','f','calls','EXTRACTED','f.py'),('d','f','calls','EXTRACTED','f.py'),
                    ('c','d','calls','EXTRACTED','f.py');",
            )
            .unwrap();
        };
        let first = assignments(&seed);
        let second = assignments(&seed);
        assert_eq!(first, second, "same input must give same communities");
    }

    #[test]
    fn higher_resolution_never_merges_more() {
        // Two triangles joined by a single bridge. Default propagation can
        // pull everything together; requiring broad neighbor agreement at
        // resolution 1.0 keeps the split visible.
        let seed = || {
            let db = open_db_in_memory().unwrap();
            db.execute_batch(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                    ('a','A','code','f.py'),('b','B','code','f.py'),('c','C','code','f.py'),
                    ('d','D','code','f.py'),('e','E','code','f.py'),('f','F','code','f.py');
                 INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                    ('a','b','calls','EXTRACTED','f.py'),('b','c','calls','EXTRACTED','f.py'),
                    ('a','c','calls','EXTRACTED','f.py'),('d','e','calls','EXTRACTED','f.py'),
                    ('e','f','calls','EXTRACTED','f.py'),('d','f','calls','EXTRACTED','f.py'),
                    ('c','d','calls','EXTRACTED','f.py');",
            )
            .unwrap();
            db
        };
        let default_count = cluster(&seed()).unwrap().communities.len();
        let fine = cluster_with(
            &seed(),
            &ClusterOptions {
                resolution: 1.0,
                exclude_hubs: false,
            },
        )
        .unwrap();
        assert!(
            fine.communities.len() >= default_count,
            "resolution 1.0 must not coarsen ({} vs {})",
            fine.communities.len(),
            default_count
        );
    }

    #[test]
    fn exclude_hubs_keeps_hubs_from_gluing_communities() {
        // A hub joined to 20 chain leaves: propagation through the hub
        // collapses everything into one community; with exclusion the two
        // chain halves survive and the hub is attached afterwards.
        let db = open_db_in_memory().unwrap();
        let mut sql = String::from(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('h','H','code','f.py'",
        );
        for i in 0..20 {
            sql.push_str(&format!("),('l{i}','L{i}','code','f.py'"));
        }
        sql.push_str(");");
        let edge = |a: &str, b: &str| {
            format!(
                "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('{a}','{b}','calls','EXTRACTED','f.py');"
            )
        };
        for i in 0..20 {
            sql.push_str(&edge("h", &format!("l{i}")));
        }
        for i in 0..9 {
            sql.push_str(&edge(&format!("l{i}"), &format!("l{}", i + 1)));
        }
        for i in 10..19 {
            sql.push_str(&edge(&format!("l{i}"), &format!("l{}", i + 1)));
        }
        db.execute_batch(&sql).unwrap();

        let plain = cluster(&db).unwrap();
        assert_eq!(
            plain.communities.len(),
            1,
            "the hub glues both halves into one community without exclusion"
        );

        let split = cluster_with(
            &db,
            &ClusterOptions {
                resolution: 0.0,
                exclude_hubs: true,
            },
        )
        .unwrap();
        assert_eq!(split.excluded_hubs, 1, "only the center is a hub here");
        let comm_of = |id: &str| -> i64 {
            db.query_row("SELECT community FROM nodes WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_ne!(
            comm_of("l0"),
            comm_of("l10"),
            "the chain halves must stay separate once the hub stops voting"
        );
        // Every node still ends up in a community, hub included.
        let unassigned: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE community IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(unassigned, 0);
    }

    #[test]
    fn exclude_hubs_off_by_default() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);
        let result = cluster(&db).unwrap();
        assert_eq!(result.excluded_hubs, 0);
        let options = ClusterOptions::default();
        assert!(!options.exclude_hubs);
        assert_eq!(options.resolution, 0.0);
    }

    #[test]
    fn modularity_recorded_and_bounded() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);
        let result = cluster(&db).unwrap();
        assert!(
            result.modularity >= -1.0 && result.modularity <= 1.0,
            "modularity must be in [-1, 1], got {}",
            result.modularity
        );
        let stored: String = db
            .query_row(
                "SELECT value FROM _meta WHERE key = 'last_modularity'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!stored.is_empty());
    }

    #[test]
    fn two_disconnected_cliques_have_high_modularity() {
        let db = open_db_in_memory().unwrap();
        // Two K4 cliques, no cross edges → near-perfect partition
        let mut sql = String::new();
        for i in 0..8 {
            sql.push_str(&format!(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('n{i}', 'N{i}', 'code', 'f.py');\n"
            ));
        }
        for i in 0..4 {
            for j in (i + 1)..4 {
                sql.push_str(&format!(
                    "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('n{i}', 'n{j}', 'calls', 'EXTRACTED', 'f.py');\n"
                ));
                sql.push_str(&format!(
                    "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('n{}', 'n{}', 'calls', 'EXTRACTED', 'f.py');\n",
                    i + 4, j + 4
                ));
            }
        }
        db.execute_batch(&sql).unwrap();
        let result = cluster(&db).unwrap();
        assert!(
            result.modularity >= 0.5,
            "two disconnected cliques should score high modularity, got {}",
            result.modularity
        );
    }
    #[test]
    fn llm_labels_survive_unchanged_recluster_and_drop_on_change() {
        use astria_core::db::community_member_hash;
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);
        cluster(&db).unwrap();

        // Simulate a prior --label-communities run on community 0.
        let mut ids: Vec<String> = db
            .prepare("SELECT id FROM nodes WHERE community = 0 ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        ids.sort();
        let refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        let hash = community_member_hash(&refs);
        db.execute(
            "UPDATE communities SET label = 'Config & Settings', summary = 'Loads settings.', label_source = 'llm', member_hash = ?1 WHERE id = 0",
            rusqlite::params![hash],
        )
        .unwrap();

        // Rebuild with identical membership: the LLM label survives.
        cluster(&db).unwrap();
        let (label, source): (String, String) = db
            .query_row(
                "SELECT label, label_source FROM communities WHERE id = 0",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (label.as_str(), source.as_str()),
            ("Config & Settings", "llm")
        );

        // Membership drifts (stale hash): the label falls back to the
        // deterministic one and provenance returns to 'source'.
        db.execute(
            "UPDATE communities SET member_hash = 'stale' WHERE id = 0",
            [],
        )
        .unwrap();
        cluster(&db).unwrap();
        let (label, source): (String, String) = db
            .query_row(
                "SELECT label, label_source FROM communities WHERE id = 0",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            source, "source",
            "stale membership must not keep an LLM label"
        );
        assert_ne!(label, "Config & Settings");
    }
}
