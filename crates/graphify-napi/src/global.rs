// global: cross-repo global graph (port of upstream global_graph.py +
// cross_repo_calls/types.py, adapted to SQLite). Repos register their graphs
// into `~/.nodesify-graphify/global.db` with a repo tag; sourced node ids are
// prefixed `<tag>::`, external/stub nodes stay unprefixed and unify by label;
// cross-repo passes add `same_type_as` and resolve parked cross-repo calls.
// All local, no LLM.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use graphify_core::GraphifyError;
use graphify_core::Result;

pub struct GlobalStore {
    pub db: Connection,
    pub path: PathBuf,
}

/// Global store location: `$HOME/.nodesify-graphify/global.db`.
pub fn global_store_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".nodesify-graphify")
        .join("global.db")
}

pub fn open_global_store() -> Result<GlobalStore> {
    open_global_store_at(&global_store_path())
}

/// Open (creating if needed) a global store at an explicit path — tests use
/// this with a tempdir so they never touch the real `~/.nodesify-graphify`.
pub fn open_global_store_at(path: &Path) -> Result<GlobalStore> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = graphify_core::db::open_db(path)?;
    Ok(GlobalStore {
        db,
        path: path.to_path_buf(),
    })
}

/// Default repo tag: the repo directory name; widened with the parent dir
/// when it collides with an existing tag (upstream `distinct_repo_tags`).
fn pick_tag(db: &Connection, repo_root: &Path, explicit: Option<&str>) -> String {
    if let Some(tag) = explicit {
        return normalize_tag(tag);
    }
    let name = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo")
        .to_string();
    let taken: Vec<String> = {
        let mut stmt = db
            .prepare("SELECT DISTINCT repo FROM nodes WHERE repo IS NOT NULL")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    if !taken.contains(&name) {
        return normalize_tag(&name);
    }
    let parent = repo_root
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("dir");
    let widened = format!("{parent}_{name}");
    if !taken.contains(&widened) {
        return normalize_tag(&widened);
    }
    let mut n = 2;
    loop {
        let candidate = format!("{}-{n}", name);
        if !taken.contains(&candidate) {
            return normalize_tag(&candidate);
        }
        n += 1;
    }
}

fn normalize_tag(tag: &str) -> String {
    graphify_core::ids::normalize_id(tag)
}

pub struct GlobalAddResult {
    pub tag: String,
    pub nodes_added: usize,
    pub edges_added: usize,
    pub same_type_edges: usize,
    pub cross_repo_call_edges: usize,
}

/// Merge one repo's graph into the global store. Re-adding a tag prunes the
/// old version first, so the operation is idempotent.
pub fn global_add(
    repo_root: &Path,
    explicit_tag: Option<&str>,
    store: &GlobalStore,
) -> Result<GlobalAddResult> {
    let repo_db_path = repo_root.join(".graphify").join("db.sqlite");
    if !repo_db_path.exists() {
        return Err(GraphifyError::Graph(format!(
            "no graph at {} — run the pipeline first",
            repo_db_path.display()
        )));
    }
    let repo_db = graphify_core::db::open_db(&repo_db_path)?;
    let tag = pick_tag(&store.db, repo_root, explicit_tag);
    // Forward-slash form to match the normalized paths stored in node rows;
    // strip Windows' extended-length `\\?\` prefix that canonicalize adds.
    let repo_root_prefix = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .display()
        .to_string()
        .replace("\\\\?\\", "")
        .replace('\\', "/");

    // Idempotency: replace this tag wholesale.
    prune_tag(&store.db, &tag)?;

    let tx = store.db.unchecked_transaction()?;

    // Sourced nodes get `<tag>::` prefixes; stubs/externals keep their ids
    // (and unify by label across repos via the label-dedup below).
    let mut external_by_label: HashMap<String, String> = HashMap::new();
    // Repo-local node id -> global id, built during the node pass so edge
    // endpoints (including stubs unified by label) resolve exactly.
    let mut local_to_global: HashMap<String, String> = HashMap::new();
    {
        let mut stmt = repo_db.prepare(
            "SELECT id, label, file_type, source_file, source_line, docstring, signature, community FROM nodes",
        )?;
        type RepoNodeRow = (
            String,
            String,
            String,
            String,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<i64>,
        );
        let rows: Vec<RepoNodeRow> = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (id, label, file_type, source_file, source_line, docstring, signature, community) in
            rows
        {
            let is_stub = file_type == "stub";
            let (global_id, final_label) = if is_stub {
                // External: unify by label — `serde_json` in two repos is one node.
                match external_by_label.get(&label.to_lowercase()) {
                    Some(existing) => (existing.clone(), label),
                    None => {
                        external_by_label.insert(label.to_lowercase(), id.clone());
                        (id.clone(), label.clone())
                    }
                }
            } else {
                (format!("{tag}::{id}"), label.clone())
            };
            local_to_global.insert(id.clone(), global_id.clone());

            // Prefix relative paths with the tag; absolute paths are
            // relativized against the repo root so ids stay readable.
            let source_file = if is_stub {
                source_file
            } else {
                let rel = source_file
                    .strip_prefix(&repo_root_prefix)
                    .map(|r| r.trim_start_matches(['\\', '/']).to_string())
                    .unwrap_or_else(|| source_file.clone());
                format!("{tag}/{rel}")
            };
            tx.execute(
                "INSERT OR IGNORE INTO nodes (id, label, file_type, source_file, source_line, docstring, signature, community, repo)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    global_id,
                    final_label,
                    file_type,
                    source_file,
                    source_line,
                    docstring,
                    signature,
                    community,
                    if is_stub { None } else { Some(tag.clone()) },
                ],
            )?;
        }
    }

    // Edges: prefix both endpoints when they belong to this repo (sourced).
    // Endpoints referencing stubs map through the label-unified external ids.
    let mut edges_added = 0usize;
    {
        let mut stmt = repo_db
            .prepare("SELECT source, target, relation, confidence, confidence_score, source_file, source_line FROM edges")?;
        type RepoEdgeRow = (
            String,
            String,
            String,
            String,
            Option<f64>,
            String,
            Option<i64>,
        );
        let rows: Vec<RepoEdgeRow> = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (source, target, relation, confidence, score, source_file, source_line) in rows {
            let gs = local_to_global.get(&source).cloned();
            let gt = local_to_global.get(&target).cloned();
            let (Some(gs), Some(gt)) = (gs, gt) else {
                continue;
            };
            if gs == gt {
                continue; // remapping collapsed a self-loop
            }
            // context stays NULL for plain merged edges — 'global' is
            // reserved for cross-repo pass output so the resolver's NOT
            // EXISTS guard can distinguish them.
            let done = tx.execute(
                "INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file, source_line)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    gs,
                    gt,
                    relation,
                    confidence,
                    score,
                    format!("{tag}/{source_file}"),
                    source_line,
                ],
            )?;
            edges_added += done as usize;
        }
    }

    tx.commit()?;

    let mut result = GlobalAddResult {
        tag,
        nodes_added: 0,
        edges_added,
        same_type_edges: 0,
        cross_repo_call_edges: 0,
    };
    {
        let count: i64 = store
            .db
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE repo = ?1",
                rusqlite::params![result.tag],
                |r| r.get(0),
            )
            .unwrap_or(0);
        result.nodes_added = count as usize;
    }

    // Cross-repo passes.
    result.same_type_edges = add_same_type_edges(store)?;
    result.cross_repo_call_edges = resolve_cross_repo_calls(store)?;
    Ok(result)
}

pub fn prune_tag(db: &Connection, tag: &str) -> Result<usize> {
    // Edges touching the repo's nodes must go first (FK: edges -> nodes).
    db.execute(
        "DELETE FROM edges WHERE source IN (SELECT id FROM nodes WHERE repo = ?1)
            OR target IN (SELECT id FROM nodes WHERE repo = ?1)",
        rusqlite::params![tag],
    )?;
    let removed = db.execute("DELETE FROM nodes WHERE repo = ?1", rusqlite::params![tag])?;
    Ok(removed)
}

pub struct GlobalListEntry {
    pub tag: String,
    pub nodes: usize,
    pub edges: usize,
}

pub fn global_list(store: &GlobalStore) -> Result<Vec<GlobalListEntry>> {
    let mut stmt = store.db.prepare(
        "SELECT n.repo, COUNT(DISTINCT n.id),
                (SELECT COUNT(*) FROM edges e JOIN nodes ns ON ns.id = e.source WHERE ns.repo = n.repo)
         FROM nodes n WHERE n.repo IS NOT NULL GROUP BY n.repo ORDER BY n.repo",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)? as usize,
            r.get::<_, i64>(2)? as usize,
        ))
    })?;
    Ok(rows
        .filter_map(|r| r.ok())
        .map(|(tag, nodes, edges)| GlobalListEntry { tag, nodes, edges })
        .collect())
}

/// `same_type_as` edges: same-label type declarations (non-function shapes)
/// across different repos. Pairwise; skips existing edges.
fn add_same_type_edges(store: &GlobalStore) -> Result<usize> {
    // Re-derive: retract the pass's own prior edges first.
    store.db.execute(
        "DELETE FROM edges WHERE context = 'global' AND relation = 'same_type_as'",
        [],
    )?;
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    {
        let mut stmt = store.db.prepare(
            "SELECT id, label FROM nodes
             WHERE repo IS NOT NULL AND file_type = 'code' AND label NOT LIKE '%()'",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for (id, label) in rows.filter_map(|r| r.ok()) {
            groups.entry(label.to_lowercase()).or_default().push(id);
        }
    }

    let mut added = 0usize;
    for (_label, ids) in groups {
        // Only labels declared in 2+ DIFFERENT repos.
        let repos: HashMap<String, String> = {
            let mut m: HashMap<String, String> = HashMap::new();
            for id in &ids {
                if let Some(repo) = repo_of(&store.db, id)? {
                    m.entry(id.clone()).or_insert(repo);
                }
            }
            m
        };
        let distinct_repos: std::collections::HashSet<&String> = repos.values().collect();
        if distinct_repos.len() < 2 {
            continue;
        }
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let (Some(ra), Some(rb)) =
                    (repo_of(&store.db, &ids[i])?, repo_of(&store.db, &ids[j])?)
                else {
                    continue;
                };
                if ra == rb {
                    continue;
                }
                let done = store.db.execute(
                    "INSERT OR IGNORE INTO edges (source, target, relation, confidence, confidence_score, source_file, context)
                     VALUES (?1, ?2, 'same_type_as', 'INFERRED', 0.9, '', 'global')",
                    rusqlite::params![ids[i], ids[j]],
                )?;
                added += done;
            }
        }
    }
    Ok(added)
}

fn repo_of(db: &Connection, id: &str) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT repo FROM nodes WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .ok())
}

/// Cross-repo call resolution: an INFERRED `calls` edge whose target is an
/// unlinked stub gains a `calls` edge to a function-shaped definition in a
/// DIFFERENT repo — only when exactly one candidate exists corpus-wide.
/// Fail closed on ambiguity. The pass deletes and re-derives its own prior
/// output, so a call resolved while unambiguous is retracted if a second
/// same-named definition appears later.
fn resolve_cross_repo_calls(store: &GlobalStore) -> Result<usize> {
    store.db.execute(
        "DELETE FROM edges WHERE context = 'global' AND relation = 'calls'",
        [],
    )?;
    // Candidates: function-shaped, sourced (repo-tagged) nodes.
    let mut candidates: HashMap<String, Vec<(String, String)>> = HashMap::new(); // bare name -> [(id, repo)]
    {
        let mut stmt = store.db.prepare(
            "SELECT id, label, repo FROM nodes
             WHERE repo IS NOT NULL AND file_type = 'code' AND label LIKE '%()'",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for (id, label, repo) in rows.filter_map(|r| r.ok()) {
            let bare = label.trim_end_matches("()").to_lowercase();
            candidates.entry(bare).or_default().push((id, repo));
        }
    }

    let mut added = 0usize;
    {
        let mut stmt = store.db.prepare(
            "SELECT e.source, e.target, tgt.label, e.source_file FROM edges e
             JOIN nodes tgt ON tgt.id = e.target
             WHERE e.relation = 'calls' AND e.confidence = 'INFERRED'
               AND tgt.file_type = 'stub'
               AND NOT EXISTS(
                 SELECT 1 FROM edges e2
                 WHERE e2.source = e.source AND e2.target = e.target AND e2.context = 'global'
               )",
        )?;
        let rows: Vec<(String, String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .filter_map(|r| r.ok())
            .collect();

        for (source, _stub_target, stub_label, source_file) in rows {
            let caller_repo = match repo_of(&store.db, &source)? {
                Some(r) => r,
                None => continue,
            };
            // Candidates are keyed by bare callable name; the stub's label may
            // carry the `()` shape or be the bare id itself.
            let bare = stub_label.trim_end_matches("()").to_lowercase();
            // Exactly one candidate in a DIFFERENT repo — fail closed.
            let matches: Vec<&(String, String)> = candidates
                .get(&bare)
                .map(|v| v.iter().filter(|(_, repo)| *repo != caller_repo).collect())
                .unwrap_or_default();
            if matches.len() != 1 {
                continue;
            }
            let (target, _) = matches[0];
            let done = store.db.execute(
                "INSERT OR IGNORE INTO edges (source, target, relation, confidence, confidence_score, source_file, context)
                 VALUES (?1, ?2, 'calls', 'INFERRED', 0.8, ?3, 'global')",
                rusqlite::params![source, target, source_file],
            )?;
            added += done;
        }
    }
    Ok(added)
}

pub fn global_remove(store: &GlobalStore, tag: &str) -> Result<usize> {
    prune_tag(&store.db, &normalize_tag(tag))
}

/// Shortest path over the global store (BFS, undirected) — the `global path`
/// command. Returns the hop chain as readable text.
pub fn global_path(store: &GlobalStore, source: &str, target: &str) -> Result<Option<String>> {
    // Resolve by id, then by unique label.
    let resolve = |name: &str| -> Result<Option<String>> {
        let exists: bool = store
            .db
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE id = ?1",
                rusqlite::params![name],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if exists {
            return Ok(Some(name.to_string()));
        }
        let mut stmt = store.db.prepare(
            "SELECT id FROM nodes WHERE LOWER(label) = LOWER(?1) GROUP BY id HAVING COUNT(*) = 1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![name], |r| r.get::<_, String>(0))?;
        if let Some(row) = rows.next() {
            return Ok(row.ok());
        }
        Ok(None)
    };

    let Some(start) = resolve(source)? else {
        return Ok(None);
    };
    let Some(goal) = resolve(target)? else {
        return Ok(None);
    };
    if start == goal {
        return Ok(Some(format!("{start} (same node)")));
    }

    // BFS over the adjacency (undirected).
    let mut parent: HashMap<String, Option<String>> = HashMap::new();
    parent.insert(start.clone(), None);
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(start.clone());
    while let Some(current) = queue.pop_front() {
        if current == goal {
            break;
        }
        let neighbors: Vec<String> = {
            let mut stmt = store.db.prepare(
                "SELECT target FROM edges WHERE source = ?1
                 UNION SELECT source FROM edges WHERE target = ?1",
            )?;
            let rows = stmt.query_map(rusqlite::params![current], |r| r.get::<_, String>(0))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        for n in neighbors {
            if !parent.contains_key(&n) {
                parent.insert(n.clone(), Some(current.clone()));
                queue.push_back(n);
            }
        }
    }

    if !parent.contains_key(&goal) {
        return Ok(None);
    }
    let mut chain = vec![goal.clone()];
    let mut current = goal.clone();
    while let Some(Some(prev)) = parent.get(&current).cloned() {
        chain.push(prev.clone());
        current = prev;
    }
    chain.reverse();
    Ok(Some(chain.join(" --")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphify_core::ids::normalize_id;

    fn seed_repo(path: &Path, crate_name: &str) -> Connection {
        let gdir = path.join(".graphify");
        std::fs::create_dir_all(&gdir).unwrap();
        let db = graphify_core::db::open_db(&gdir.join("db.sqlite")).unwrap();
        let id = normalize_id(crate_name);
        db.execute_batch(&format!(
            r#"
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('{id}_src_main', 'main.rs', 'file', 'src/main.rs'),
              ('{id}_api', '{crate_name}Api', 'code', 'src/main.rs'),
              ('{id}_api_send', 'send()', 'code', 'src/main.rs'),
              ('{id}_config', 'Config', 'code', 'src/config.rs'),
              ('serde_json', 'serde_json', 'stub', 'deps');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('{id}_src_main', '{id}_api', 'contains', 'EXTRACTED', 'src/main.rs'),
              ('{id}_api', '{id}_api_send', 'contains', 'EXTRACTED', 'src/main.rs'),
              ('{id}_src_main', '{id}_config', 'contains', 'EXTRACTED', 'src/config.rs'),
              ('{id}_api_send', 'serde_json', 'calls', 'INFERRED', 'src/main.rs');
            "#
        ))
        .unwrap();
        db
    }

    #[test]
    fn global_add_prefixes_and_unifies_externals() {
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("alpha");
        let repo_b = dir.path().join("beta");
        let db_a = seed_repo(&repo_a, "alpha");
        let db_b = seed_repo(&repo_b, "beta");
        drop(db_a);
        drop(db_b);

        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        let res_a = global_add(&repo_a, Some("alpha"), &store).unwrap();
        let res_b = global_add(&repo_b, Some("beta"), &store).unwrap();
        assert_eq!(res_a.tag, "alpha");
        // Same-name type across repos -> same_type_as edge.
        assert!(res_b.same_type_edges >= 1);
        // External `serde_json` unified: ONE stub node, two repo edges.
        let serde_count: i64 = store
            .db
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE id = 'serde_json'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(serde_count, 1);
        let serde_edges: i64 = store
            .db
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE target = 'serde_json'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_edges, 2,
            "both repos' calls edges hit one unified node"
        );
        let listing = global_list(&store).unwrap();
        assert_eq!(listing.len(), 2);
    }

    #[test]
    fn re_add_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("alpha");
        let db_a = seed_repo(&repo_a, "alpha");
        drop(db_a);
        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        global_add(&repo_a, Some("alpha"), &store).unwrap();
        let first: i64 = store
            .db
            .query_row("SELECT COUNT(*) FROM nodes WHERE repo = 'alpha'", [], |r| {
                r.get(0)
            })
            .unwrap();
        global_add(&repo_a, Some("alpha"), &store).unwrap();
        let second: i64 = store
            .db
            .query_row("SELECT COUNT(*) FROM nodes WHERE repo = 'alpha'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn cross_repo_call_resolves_single_candidate() {
        let dir = tempfile::tempdir().unwrap();
        // Repo A defines a bare function handler() and a STUB call target send_x().
        let repo_a = dir.path().join("caller");
        let gdir_a = repo_a.join(".graphify");
        std::fs::create_dir_all(&gdir_a).unwrap();
        let db_a = graphify_core::db::open_db(&gdir_a.join("db.sqlite")).unwrap();
        db_a.execute_batch(
            r#"
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('caller_main', 'main.rs', 'file', 'src/main.rs'),
              ('handler', 'handler()', 'code', 'src/main.rs'),
              ('call_sendx', 'call_sendx()', 'code', 'src/main.rs'),
              ('sendx', 'send_x()', 'stub', 'unresolved');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('caller_main', 'call_sendx', 'contains', 'EXTRACTED', 'src/main.rs'),
              ('call_sendx', 'sendx', 'calls', 'INFERRED', 'src/main.rs');
            "#,
        )
        .unwrap();
        drop(db_a);
        // Repo B defines send_x() as a sourced function.
        let repo_b = dir.path().join("callee");
        let gdir_b = repo_b.join(".graphify");
        std::fs::create_dir_all(&gdir_b).unwrap();
        let db_b = graphify_core::db::open_db(&gdir_b.join("db.sqlite")).unwrap();
        db_b.execute_batch(
            r#"
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('callee_send', 'send_x()', 'code', 'src/send.rs');
            "#,
        )
        .unwrap();
        drop(db_b);

        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        let res_a = global_add(&repo_a, Some("caller"), &store).unwrap();
        let res_b = global_add(&repo_b, Some("callee"), &store).unwrap();
        assert_eq!(res_a.cross_repo_call_edges + res_b.cross_repo_call_edges, 1);
        let target: String = store
            .db
            .query_row(
                "SELECT target FROM edges WHERE context = 'global' AND relation = 'calls'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(target, "callee::callee_send");
    }

    #[test]
    fn ambiguous_call_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("caller");
        let gdir_a = repo_a.join(".graphify");
        std::fs::create_dir_all(&gdir_a).unwrap();
        let db_a = graphify_core::db::open_db(&gdir_a.join("db.sqlite")).unwrap();
        db_a.execute_batch(
            r#"
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('c1', 'call_x()', 'code', 'src/a.rs'),
              ('sendx', 'send_x()', 'stub', 'unresolved');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('c1', 'sendx', 'calls', 'INFERRED', 'src/a.rs');
            "#,
        )
        .unwrap();
        drop(db_a);
        // TWO repos define send_x() -> ambiguous -> no edge.
        for name in ["one", "two"] {
            let repo = dir.path().join(name);
            let gdir = repo.join(".graphify");
            std::fs::create_dir_all(&gdir).unwrap();
            let db = graphify_core::db::open_db(&gdir.join("db.sqlite")).unwrap();
            db.execute_batch(&format!(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('{name}_sx', 'send_x()', 'code', 'src/s.rs');"
            ))
            .unwrap();
            drop(db);
        }
        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        let res = global_add(&repo_a, Some("caller"), &store).unwrap();
        global_add(&dir.path().join("one"), Some("one"), &store).unwrap();
        let res_after_one = res.cross_repo_call_edges;
        global_add(&dir.path().join("two"), Some("two"), &store).unwrap();
        // At no point may the ambiguous call resolve.
        let resolved: i64 = store
            .db
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE context = 'global' AND relation = 'calls' AND source LIKE 'caller::%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            resolved, 0,
            "ambiguous call must stay unresolved (had {res_after_one} during single-repo phase)"
        );
    }
}
