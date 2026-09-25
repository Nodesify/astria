// affected: reverse-reachability "blast radius" analysis.
// Answers: "what breaks if I change X?" — walks incoming edges (callers,
// importers, inheritors, users) outward from a seed node.
// Ported from upstream astria v8 affected.py.

use rusqlite::Connection;
use std::collections::{HashMap, HashSet, VecDeque};

use astria_core::AstriaError;

/// Relations that propagate impact backwards. Structural relations
/// (contains/method) are used only for the member-seed hop, not for
/// blast-radius expansion — a class containing a method is not "affected"
/// by it in the way a caller is.
const IMPACT_RELATIONS: &[&str] = &[
    "calls",
    "references",
    "imports",
    "imports_from",
    "inherits",
    "uses",
    "depends_on",
    "requires",
];

#[derive(Debug, Clone)]
pub struct AffectedHit {
    pub id: String,
    pub label: String,
    pub depth: u32,
    /// Relation of the edge that reached this node (e.g. the caller's `calls`).
    pub relation: String,
    /// File containing the referencing edge — the call site, not the definition.
    pub via_file: String,
}

#[derive(Debug, Clone)]
pub struct AffectedResult {
    pub seed: String,
    pub seed_label: String,
    pub total: usize,
    pub hits: Vec<AffectedHit>,
}

struct NodeInfo {
    label: String,
    source_file: String,
}

/// Resolve a user-supplied query to a node id.
/// Order: exact id → exact label → bare name (label without `()`/leading `.`)
/// → unique case-insensitive label → unique source-file suffix match.
fn resolve_seed(db: &Connection, query: &str) -> astria_core::Result<String> {
    let exact: Option<String> = db
        .query_row(
            "SELECT id FROM nodes WHERE id = ?1",
            rusqlite::params![query],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = exact {
        return Ok(id);
    }

    let exact_label: Option<String> = db
        .query_row(
            "SELECT id FROM nodes WHERE label = ?1 LIMIT 2",
            rusqlite::params![query],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = exact_label {
        return Ok(id);
    }

    // Method labels look like ".foo()" — accept the bare name too.
    let bare = query
        .trim_start_matches('.')
        .trim_end_matches("()")
        .to_lowercase();
    let bare_hit: Option<String> = db
        .query_row(
            "SELECT id FROM nodes WHERE lower(replace(ltrim(label, '.'), '()', '')) = ?1 LIMIT 2",
            rusqlite::params![bare],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = bare_hit {
        return Ok(id);
    }

    let lower = query.to_lowercase();
    let mut stmt = db.prepare("SELECT id, source_file FROM nodes WHERE lower(label) = ?1")?;
    let candidates: Vec<(String, String)> = stmt
        .query_map(rusqlite::params![lower], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    match candidates.len() {
        1 => return Ok(candidates[0].0.clone()),
        n if n > 1 => {
            return Err(AstriaError::Graph(format!(
                "ambiguous node '{query}' — {n} nodes share that label; use one of: {}",
                describe_candidates(&candidates)
            )))
        }
        _ => {}
    }

    // Source-file suffix: "src/lib.rs" or "lib.rs" should find the file node.
    let norm_query = query.replace('\\', "/");
    let suffix = format!("%{}", norm_query.trim_start_matches('/'));
    let mut stmt = db.prepare("SELECT id, source_file FROM nodes WHERE source_file LIKE ?1")?;
    let candidates: Vec<(String, String)> = stmt
        .query_map(rusqlite::params![suffix], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    match candidates.len() {
        1 => Ok(candidates[0].0.clone()),
        n if n > 1 => Err(AstriaError::Graph(format!(
            "ambiguous path '{query}' — {n} nodes match; use one of: {}",
            describe_candidates(&candidates)
        ))),
        _ => Err(AstriaError::Graph(format!("node not found: '{query}'"))),
    }
}

/// Render ambiguous matches as actionable `id (file)` options so an agent
/// can retry with an exact id instead of exploring.
fn describe_candidates(candidates: &[(String, String)]) -> String {
    candidates
        .iter()
        .take(5)
        .map(|(id, file)| {
            if file.is_empty() {
                id.clone()
            } else {
                format!("{id} ({file})")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Compute the blast radius of `query`: everything that references it,
/// directly or transitively, up to `depth` hops. `relation_filter`
/// restricts traversal to a single relation (e.g. "calls").
pub fn affected(
    db: &Connection,
    query: &str,
    depth: u32,
    relation_filter: Option<&str>,
) -> astria_core::Result<AffectedResult> {
    let seed = resolve_seed(db, query)?;

    let mut nodes: HashMap<String, NodeInfo> = HashMap::new();
    {
        let mut stmt = db.prepare("SELECT id, label, source_file FROM nodes")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                NodeInfo {
                    label: r.get::<_, String>(1)?,
                    source_file: r.get::<_, String>(2)?,
                },
            ))
        })?;
        for (id, info) in rows.flatten() {
            nodes.insert(id, info);
        }
    }

    let allowed: HashSet<&str> = match relation_filter {
        Some(f) => {
            let set: HashSet<&str> = IMPACT_RELATIONS.iter().copied().collect();
            if !set.contains(f) {
                return Err(AstriaError::Graph(format!(
                    "unsupported relation '{f}' — allowed: {}",
                    IMPACT_RELATIONS.join(", ")
                )));
            }
            [f].into_iter().collect()
        }
        None => IMPACT_RELATIONS.iter().copied().collect(),
    };

    // Reverse adjacency: target → [(source, relation, edge source_file)].
    // Built once so the BFS is O(V + E) instead of rescanning every edge
    // per frontier node.
    let mut incoming: HashMap<String, Vec<(String, String, String)>> = HashMap::new();
    // Members of the seed (seed --contains/method--> member), for the
    // one-hop seed expansion below.
    let mut seed_members: Vec<String> = Vec::new();
    {
        let mut stmt = db.prepare("SELECT source, target, relation, source_file FROM edges")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for (src, tgt, rel, via) in rows.flatten() {
            if src == seed && (rel == "contains" || rel == "method") {
                seed_members.push(tgt.clone());
            }
            incoming.entry(tgt).or_default().push((src, rel, via));
        }
    }

    let seed_label = nodes
        .get(&seed)
        .map(|n| n.label.clone())
        .unwrap_or_else(|| seed.clone());

    // Seed set: the node itself, its members (one contains/method hop, so
    // callers of a class's methods are found), and its bare-name alias —
    // call edges land on bare stub ids ("load_graph_db"), while definitions
    // have qualified ids ("src_pipeline::load_graph_db").
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(seed.clone());
    let mut frontier: VecDeque<(String, u32)> = VecDeque::new();
    frontier.push_back((seed.clone(), 0));

    if let Some(bare) = seed.rsplit("::").next() {
        if bare != seed && nodes.contains_key(bare) {
            visited.insert(bare.to_string());
            frontier.push_back((bare.to_string(), 0));
        }
    }

    for tgt in seed_members {
        if visited.insert(tgt.clone()) {
            frontier.push_back((tgt, 0));
        }
    }

    let mut hits: Vec<AffectedHit> = Vec::new();
    while let Some((current, d)) = frontier.pop_front() {
        if d >= depth {
            continue;
        }
        let empty = Vec::new();
        for (src, rel, via) in incoming.get(&current).unwrap_or(&empty) {
            if !allowed.contains(rel.as_str()) || !visited.insert(src.clone()) {
                continue;
            }
            let label = nodes
                .get(src)
                .map(|n| n.label.clone())
                .unwrap_or_else(|| src.clone());
            let via_file = if via.is_empty() {
                nodes
                    .get(src)
                    .map(|n| n.source_file.clone())
                    .unwrap_or_default()
            } else {
                via.clone()
            };
            hits.push(AffectedHit {
                id: src.clone(),
                label,
                depth: d + 1,
                relation: rel.clone(),
                via_file,
            });
            frontier.push_back((src.clone(), d + 1));
        }
    }

    hits.sort_by(|a, b| a.depth.cmp(&b.depth).then_with(|| a.label.cmp(&b.label)));

    Ok(AffectedResult {
        seed,
        seed_label,
        total: hits.len(),
        hits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    fn seed_graph(db: &Connection) {
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('svc', 'Service', 'code', 'src/svc.rs'),
              ('svc_run', '.run()', 'code', 'src/svc.rs'),
              ('handler', 'Handler', 'code', 'src/handler.rs'),
              ('route', 'build_route', 'code', 'src/route.rs'),
              ('main', 'main', 'code', 'src/main.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('svc', 'svc_run', 'contains', 'EXTRACTED', 'src/svc.rs'),
              ('handler', 'svc_run', 'calls', 'EXTRACTED', 'src/handler.rs'),
              ('route', 'handler', 'calls', 'EXTRACTED', 'src/route.rs'),
              ('main', 'route', 'calls', 'EXTRACTED', 'src/main.rs'),
              ('main', 'svc', 'imports', 'EXTRACTED', 'src/main.rs');
        ",
        )
        .unwrap();
    }

    #[test]
    fn finds_direct_callers_via_member_seed_hop() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);
        // Seeding the class finds the caller of its method (depth 1)
        let result = affected(&db, "Service", 2, None).unwrap();
        assert_eq!(result.seed, "svc");
        let d1: Vec<&AffectedHit> = result.hits.iter().filter(|h| h.depth == 1).collect();
        assert!(d1
            .iter()
            .any(|h| h.id == "handler" && h.relation == "calls"));
    }

    #[test]
    fn transitive_callers_respect_depth() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);

        let shallow = affected(&db, "svc_run", 1, None).unwrap();
        assert!(shallow.hits.iter().any(|h| h.id == "handler"));
        assert!(!shallow.hits.iter().any(|h| h.id == "main"));

        let deep = affected(&db, "svc_run", 2, None).unwrap();
        assert!(deep.hits.iter().any(|h| h.id == "handler" && h.depth == 1));
        assert!(deep.hits.iter().any(|h| h.id == "route" && h.depth == 2));
        assert!(!deep.hits.iter().any(|h| h.id == "main")); // 3 hops away

        let deeper = affected(&db, "svc_run", 3, None).unwrap();
        assert!(deeper.hits.iter().any(|h| h.id == "main" && h.depth == 3));
    }

    #[test]
    fn relation_filter_excludes_importers() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);
        let result = affected(&db, "svc", 2, Some("calls")).unwrap();
        // main only imports svc — filtered out
        assert!(!result
            .hits
            .iter()
            .any(|h| h.id == "main" && h.relation == "imports"));
        assert!(result.hits.iter().all(|h| h.relation == "calls"));
    }

    #[test]
    fn seed_resolution_by_bare_name_and_path() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);

        let by_bare = affected(&db, "run", 1, None).unwrap();
        assert_eq!(by_bare.seed, "svc_run");

        let by_path = affected(&db, "src/route.rs", 1, None).unwrap();
        assert_eq!(by_path.seed, "route");
    }

    #[test]
    fn unknown_node_errors() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);
        let err = affected(&db, "DoesNotExist", 1, None).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn bare_alias_stub_is_seeded() {
        // Definitions have qualified ids; call edges land on bare stubs —
        // the blast radius of a definition must include the stub's callers.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('src_x::run', 'run()', 'code', 'x.rs'),
              ('run', 'run', 'stub', ''),
              ('caller', 'caller()', 'code', 'y.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('caller', 'run', 'calls', 'EXTRACTED', 'y.rs');
        ",
        )
        .unwrap();
        let result = affected(&db, "src_x::run", 1, None).unwrap();
        assert!(result.hits.iter().any(|h| h.id == "caller"));
    }

    #[test]
    fn via_file_reports_call_site() {
        let db = open_db_in_memory().unwrap();
        seed_graph(&db);
        let result = affected(&db, "svc_run", 1, None).unwrap();
        let handler = result.hits.iter().find(|h| h.id == "handler").unwrap();
        assert_eq!(handler.via_file, "src/handler.rs");
    }
}
