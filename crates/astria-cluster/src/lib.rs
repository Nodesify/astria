// astria-cluster: label propagation clustering with thematic community
// labels (distinctive-term naming, hub fallback) and oversized-community
// splitting (ported from upstream astria v8).

use petgraph::graph::NodeIndex;
use petgraph::graph::UnGraph;
use rusqlite::Connection;
use std::collections::HashMap;

/// Communities larger than 25% of the graph (and at least OVERSIZED_MIN
/// nodes) are re-partitioned — one giant community swallows the report.
const OVERSIZED_SHARE: f64 = 0.25;
const OVERSIZED_MIN: usize = 10;

#[derive(Debug)]
pub struct ClusterResult {
    pub communities: HashMap<u32, usize>,
    /// Thematic, LLM-free labels: each community is named after its most
    /// distinctive term ("Extraction", "Similarity") when one concentrates
    /// inside it, falling back to the highest-degree member's name.
    pub labels: HashMap<u32, String>,
    pub iterations: u32,
    /// Newman modularity of the final partition in [-1, 1].
    pub modularity: f64,
}

pub fn cluster(db: &Connection) -> astria_core::Result<ClusterResult> {
    // Load nodes (id + label for hub naming). Ordered by id so label
    // propagation is deterministic across runs and platforms.
    let node_ids: Vec<String> = {
        let mut stmt = db.prepare("SELECT id FROM nodes ORDER BY id")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    let node_labels: HashMap<String, String> = {
        let mut stmt = db.prepare("SELECT id, label FROM nodes")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    if node_ids.is_empty() {
        return Ok(ClusterResult {
            communities: HashMap::new(),
            labels: HashMap::new(),
            iterations: 0,
            modularity: 0.0,
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
            .filter_map(|r| r.ok())
            .collect();
        for (src, tgt) in edges {
            if let (Some(&s), Some(&t)) = (id_to_idx.get(&src), id_to_idx.get(&tgt)) {
                graph.add_edge(s, t, ());
            }
        }
    }

    // Label propagation
    let n = node_ids.len();
    let mut labels: Vec<u32> = (0..n as u32).collect();
    let iterations = propagate(&graph, &mut labels);

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
            propagate(&sub_graph, &mut sub_labels);
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

    // Hub-based labels and cohesion, persisted for the report and exports
    let mut hub_labels: HashMap<u32, String> = HashMap::new();
    let mut hub_degree: HashMap<u32, usize> = HashMap::new();
    let mut internal_edges: HashMap<u32, usize> = HashMap::new();
    let mut boundary_edges: HashMap<u32, usize> = HashMap::new();
    let mut cohesion: HashMap<u32, f64> = HashMap::new();

    for (i, id) in node_ids.iter().enumerate() {
        let c = labels[i];
        let degree = graph.neighbors(NodeIndex::new(i)).count();
        if hub_degree.get(&c).copied().unwrap_or(0) < degree {
            hub_degree.insert(c, degree);
            hub_labels.insert(
                c,
                node_labels
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| format!("Community {c}")),
            );
        }
    }
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

    let final_labels = thematic_labels(&node_ids, &labels, &node_labels, &hub_labels);

    // LLM enrichment from the previous build: a community whose membership
    // is unchanged keeps its `--label-communities` name/summary across
    // rebuilds; a changed membership falls back to the fresh thematic/hub
    // label (a stale LLM name for a different group would be a lie).
    let previous_enrichment: HashMap<i64, (String, Option<String>, String, Option<String>)> = db
        .prepare("SELECT id, label, summary, label_source, member_hash FROM communities")
        .map(|mut stmt| {
            stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })
            .map(|rows| {
                rows.filter_map(|r| r.ok())
                    .map(|(id, label, summary, source, hash)| (id, (label, summary, source, hash)))
                    .collect()
            })
            .unwrap_or_default()
        })
        .unwrap_or_default();

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
            let preserved = hash.and_then(|h| {
                previous_enrichment.get(&(c as i64)).and_then(
                    |(label, summary, source, old_hash)| {
                        if source == "llm" && old_hash.as_deref() == Some(h) {
                            Some((label.clone(), summary.clone()))
                        } else {
                            None
                        }
                    },
                )
            });
            let (label, summary, label_source) = match preserved {
                Some((label, summary)) => (label, summary, "llm".to_string()),
                None => (
                    final_labels
                        .get(&c)
                        .cloned()
                        .unwrap_or_else(|| format!("Community {c}")),
                    None,
                    "hub".to_string(),
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
    // thematic ones.
    let labels: HashMap<u32, String> = {
        let mut stmt = db.prepare("SELECT id, label FROM communities")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .filter_map(|r| r.ok())
            .collect::<Vec<(i64, String)>>();
        rows.into_iter()
            .map(|(id, label)| (id as u32, label))
            .collect()
    };

    Ok(ClusterResult {
        communities,
        labels,
        iterations,
        modularity,
    })
}

// ---------------------------------------------------------------------------
// Thematic community labels
// ---------------------------------------------------------------------------

/// Terms shorter/longer than this never name a community.
const MIN_TERM_LEN: usize = 3;
const MAX_TERM_LEN: usize = 24;
/// A term must appear on at least this many members to be a label candidate.
const MIN_TERM_SUPPORT: usize = 3;
/// And at least this share of its uses graph-wide must sit in the community.
const MIN_CONCENTRATION: f64 = 0.5;
/// Generic code/structure tokens that appear everywhere and never theme a
/// community; concentration alone would mostly handle these, this trims the
/// obvious noise cheaply.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "this", "that", "into", "not", "are", "test", "tests",
    "spec", "src", "lib", "main", "index", "new", "get", "set", "std", "self", "impl", "pub", "fn",
    "let", "mut", "some", "none", "true", "false", "http", "https", "www", "sample", "example",
    "unknown",
];

/// Split a label into lowercase word terms: non-alphanumeric separators and
/// camelCase boundaries both break words ("export_wiki" → [export, wiki],
/// "buildPayload" → [build, payload]).
fn tokenize_label(label: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for ch in label.chars() {
        if ch.is_alphanumeric() {
            let upper = ch.is_uppercase();
            if !current.is_empty() && upper && prev_lower {
                terms.push(std::mem::take(&mut current));
            }
            current.push(ch);
            prev_lower = !upper;
        } else if !current.is_empty() {
            terms.push(std::mem::take(&mut current));
            prev_lower = false;
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
        .into_iter()
        .map(|t| t.to_lowercase())
        .filter(|t| (MIN_TERM_LEN..=MAX_TERM_LEN).contains(&t.len()))
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// Name each community after its most distinctive term — the word that
/// concentrates inside it relative to the whole graph — instead of the
/// highest-degree member's name ("get()", "lib.rs"). Communities without a
/// qualifying term keep their deterministic hub label. Pure function of the
/// graph, so labels stay deterministic across runs.
fn thematic_labels(
    node_ids: &[String],
    labels: &[u32],
    node_labels: &HashMap<String, String>,
    hub_labels: &HashMap<u32, String>,
) -> HashMap<u32, String> {
    let mut by_community: HashMap<u32, HashMap<String, usize>> = HashMap::new();
    let mut community_sizes: HashMap<u32, usize> = HashMap::new();
    let mut global: HashMap<String, usize> = HashMap::new();
    for (i, id) in node_ids.iter().enumerate() {
        let c = labels[i];
        *community_sizes.entry(c).or_insert(0) += 1;
        if let Some(label) = node_labels.get(id) {
            for term in tokenize_label(label) {
                *by_community
                    .entry(c)
                    .or_default()
                    .entry(term.clone())
                    .or_insert(0) += 1;
                *global.entry(term).or_insert(0) += 1;
            }
        }
    }

    let mut out: HashMap<u32, String> = HashMap::new();
    let mut community_ids: Vec<u32> = community_sizes.keys().copied().collect();
    community_ids.sort_unstable();
    for c in community_ids {
        let size = community_sizes[&c];
        // Minimum support scales with community size so the label describes
        // the community, not a pair of members sharing a helper name.
        let support_floor = (size / 20).clamp(MIN_TERM_SUPPORT, 10);
        let mut best: Option<(usize, f64, String)> = None;
        if let Some(terms) = by_community.get(&c) {
            for (term, &count) in terms {
                if count < support_floor {
                    continue;
                }
                let concentration = count as f64 / global[term].max(1) as f64;
                if concentration < MIN_CONCENTRATION {
                    continue;
                }
                let score = count as f64 * concentration;
                let candidate = (count, score, term.clone());
                let better = match &best {
                    None => true,
                    Some((b_count, b_score, b_term)) => {
                        score > *b_score
                            || (score == *b_score
                                && (count > *b_count || (count == *b_count && term < b_term)))
                    }
                };
                if better {
                    best = Some(candidate);
                }
            }
        }
        let label = match best {
            Some((_, _, term)) => {
                let mut chars = term.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => term,
                }
            }
            None => hub_labels
                .get(&c)
                .cloned()
                .unwrap_or_else(|| format!("Community {c}")),
        };
        out.insert(c, label);
    }
    out
}

/// One full label-propagation pass loop. Returns iterations used.
///
/// Tie-breaking is deterministic: among labels with the maximum neighbor
/// count, the smallest label id wins. Rust's HashMap iteration order is
/// randomized per process, so `max_by_key` alone would make communities
/// (and every downstream report) differ between runs.
fn propagate(graph: &UnGraph<String, ()>, labels: &mut [u32]) -> u32 {
    let n = labels.len();
    let mut iterations = 0;
    for _ in 0..100 {
        iterations += 1;
        let mut changed = false;
        for i in 0..n {
            let node_idx = NodeIndex::new(i);
            let mut neighbor_labels: HashMap<u32, usize> = HashMap::new();
            neighbor_labels.insert(labels[i], 1);
            for neighbor in graph.neighbors(node_idx) {
                *neighbor_labels.entry(labels[neighbor.index()]).or_insert(0) += 1;
            }
            let best_label = {
                let max_count = neighbor_labels.values().copied().max().unwrap_or(0);
                // Smallest label id among the maxima: fully deterministic
                // (HashMap iteration order must not decide communities).
                neighbor_labels
                    .iter()
                    .filter(|(_, &count)| count == max_count)
                    .map(|(&label, _)| label)
                    .min()
                    .unwrap_or(labels[i])
            };
            if best_label != labels[i] {
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
    fn hub_labels_written_to_communities_table() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);
        let result = cluster(&db).unwrap();

        // Every community gets a label from its highest-degree member
        assert_eq!(result.labels.len(), result.communities.len());
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM communities", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count as usize, result.communities.len());
        // Labels come from real node labels, not placeholders
        let rows: Vec<(i64, String)> = db
            .prepare("SELECT id, label FROM communities")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for (_, label) in rows {
            assert!(["A", "B", "C", "D"].contains(&label.as_str()));
        }
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
    fn thematic_label_from_distinctive_term() {
        let db = open_db_in_memory().unwrap();
        // One connected community whose labels share the term "extract",
        // plus two singletons sharing "query" — 2 uses is below the support
        // floor, so the singletons must keep their hub labels.
        let mut sql = String::from(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('e1','extract_file()','code','f.py'),('e2','extract_dir()','code','f.py'),
                ('e3','extract_root()','code','f.py'),('e4','extract_module()','code','f.py'),
                ('e5','extract_single()','code','f.py'),('e6','walker()','code','f.py'),
                ('e7','parser()','code','f.py'),('e8','reader()','code','f.py'),
                ('q1','query_parse()','code','f.py'),('q2','query_plan()','code','f.py');\n",
        );
        for i in 1..8 {
            sql.push_str(&format!(
                "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('e{i}', 'e{}', 'calls', 'EXTRACTED', 'f.py');\n",
                i + 1
            ));
        }
        db.execute_batch(&sql).unwrap();
        cluster(&db).unwrap();

        let (big_label, big_size): (String, i64) = db
            .query_row(
                "SELECT label, size FROM communities ORDER BY size DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(big_size, 8);
        assert_eq!(
            big_label, "Extract",
            "distinctive term should name the community"
        );

        let singleton_labels: Vec<String> = db
            .prepare("SELECT label FROM communities WHERE size = 1")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(
            singleton_labels.len(),
            2,
            "both singletons should form their own community"
        );
        for label in singleton_labels {
            assert_ne!(
                label, "Query",
                "shared term below the support floor must not name the community"
            );
        }
    }

    #[test]
    fn no_candidates_is_noop_fallback_hub_labels() {
        // Single-letter labels never yield qualifying terms: every community
        // must fall back to its hub member's name (the pre-thematic behavior).
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);
        let result = cluster(&db).unwrap();
        for label in result.labels.values() {
            assert!(
                ["A", "B", "C", "D"].contains(&label.as_str()),
                "fallback must be the hub label, got {label}"
            );
        }
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
        // deterministic one and provenance returns to 'hub'.
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
        assert_eq!(source, "hub", "stale membership must not keep an LLM label");
        assert_ne!(label, "Config & Settings");
    }
}
