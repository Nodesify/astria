// global: cross-repo global graph (port of upstream global_graph.py +
// cross_repo_calls/types.py, adapted to SQLite). Repos register their graphs
// into `~/.astria/global.db` with a repo tag; sourced node ids are
// prefixed `<tag>::`, external/stub nodes stay unprefixed and unify by label;
// cross-repo passes add `same_type_as` and resolve parked cross-repo calls.
// All local, no LLM.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use astria_core::AstriaError;
use astria_core::Result;

pub struct GlobalStore {
    pub db: Connection,
    pub path: PathBuf,
}

/// Global store location: `$HOME/.astria/global.db`.
pub fn global_store_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".astria").join("global.db")
}

pub fn open_global_store() -> Result<GlobalStore> {
    open_global_store_at(&global_store_path())
}

/// Open (creating if needed) a global store at an explicit path — tests use
/// this with a tempdir so they never touch the real `~/.astria`.
pub fn open_global_store_at(path: &Path) -> Result<GlobalStore> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = astria_core::db::open_db(path)?;
    Ok(GlobalStore {
        db,
        path: path.to_path_buf(),
    })
}

/// Canonical registration key for a repo root: forward-slash absolute path
/// (the same normalization the node rows use).
fn root_key(repo_root: &Path) -> String {
    repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .display()
        .to_string()
        .replace("\\\\?\\", "")
        .replace('\\', "/")
}

/// Persisted root→tag registrations: repeat additions of the same root
/// update its own tag instead of allocating a duplicate, and an explicit
/// tag owned by a different root is an error, never a silent replacement.
/// Created lazily so existing stores adopt it on next open.
fn ensure_roots_table(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS global_roots (
            root TEXT PRIMARY KEY,
            tag TEXT NOT NULL,
            registered_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS global_snapshots (
            tag TEXT PRIMARY KEY,
            source_commit TEXT,
            graph_generation TEXT,
            graph_built_at TEXT
        )",
    )?;
    Ok(())
}

fn registered_tag(db: &Connection, root: &str) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT tag FROM global_roots WHERE root = ?1",
            rusqlite::params![root],
            |r| r.get(0),
        )
        .ok())
}

/// Who owns a tag: the registered root, `Some("")` when the tag has nodes
/// but no registration (stores predating the registry), or `None` when free.
fn tag_owner(db: &Connection, tag: &str) -> Result<Option<String>> {
    if let Ok(root) = db.query_row(
        "SELECT root FROM global_roots WHERE tag = ?1",
        rusqlite::params![tag],
        |r| r.get::<_, String>(0),
    ) {
        return Ok(Some(root));
    }
    let used: i64 = db.query_row(
        "SELECT COUNT(*) FROM nodes WHERE repo = ?1",
        rusqlite::params![tag],
        |r| r.get(0),
    )?;
    Ok(if used > 0 { Some(String::new()) } else { None })
}

/// Resolve which tag this addition writes:
/// - A registered root reuses its own tag (explicit `--as` must agree).
/// - A new root with an explicit tag claims it, erroring when another root
///   owns that tag — never pruning a foreign graph.
/// - A new root without `--as` picks the first free candidate; every
///   candidate is normalized BEFORE the collision check, so `Foo-Bar` and
///   `foo_bar` cannot converge onto an existing tag.
fn resolve_tag(db: &Connection, repo_root: &Path, explicit: Option<&str>) -> Result<String> {
    ensure_roots_table(db)?;
    let root = root_key(repo_root);
    if let Some(existing) = registered_tag(db, &root)? {
        match explicit {
            Some(want) => {
                let want = normalize_tag(want);
                if want == existing {
                    return Ok(existing);
                }
                return Err(AstriaError::Graph(format!(
                    "root {} is already registered as tag '{existing}'; remove it first (`global remove {existing}`) to re-register as '{want}'",
                    repo_root.display()
                )));
            }
            None => return Ok(existing),
        }
    }

    if let Some(want) = explicit {
        let want = normalize_tag(want);
        return match tag_owner(db, &want)? {
            // Registered to someone else, or in use by an unknown owner:
            // replacing either silently would delete another root's graph.
            Some(other) => Err(AstriaError::Graph(format!(
                "tag '{want}' is already in use{}; choose another with --as",
                if other.is_empty() {
                    String::new()
                } else {
                    format!(" by {other}")
                }
            ))),
            None => Ok(want),
        };
    }

    let name = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo");
    let parent = repo_root
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("dir");
    // Check each candidate AS IT IS BUILT: the previous shape pushed a
    // thousand candidates and returned an error before ever testing them,
    // so every registration without --as failed.
    for candidate in [
        normalize_tag(name),
        normalize_tag(&format!("{parent}_{name}")),
    ] {
        if tag_owner(db, &candidate)?.is_none() {
            return Ok(candidate);
        }
    }
    for n in 2..=1000 {
        let candidate = normalize_tag(&format!("{name}-{n}"));
        if tag_owner(db, &candidate)?.is_none() {
            return Ok(candidate);
        }
    }
    Err(AstriaError::Graph(format!(
        "no free tag for {} after 1000 attempts",
        repo_root.display()
    )))
}

fn normalize_tag(tag: &str) -> String {
    astria_core::ids::normalize_id(tag)
}

#[derive(Debug)]
pub struct GlobalAddResult {
    pub tag: String,
    pub nodes_added: usize,
    pub edges_added: usize,
    pub same_type_edges: usize,
    pub cross_repo_call_edges: usize,
}

/// Merge one repo's graph into the global store. Re-adding a registered
/// root updates its own tag; the prune of the previous version happens
/// inside the same transaction as the replacement, so a failed re-add
/// rolls back to the prior complete graph instead of losing it.
pub fn global_add(
    repo_root: &Path,
    explicit_tag: Option<&str>,
    store: &GlobalStore,
) -> Result<GlobalAddResult> {
    let _writer = astria_core::writer_lock::WriterLock::acquire_in(
        store.path.parent().unwrap_or_else(|| Path::new(".")),
    )?;
    let repo_db_path = repo_root.join(".astria").join("db.sqlite");
    if !repo_db_path.exists() {
        return Err(AstriaError::Graph(format!(
            "no graph at {} — run the pipeline first",
            repo_db_path.display()
        )));
    }
    let repo_db = astria_core::db::open_db(&repo_db_path)?;
    let _source_snapshot = repo_db.unchecked_transaction()?;
    let source_meta = |key: &str| -> Option<String> {
        repo_db
            .query_row("SELECT value FROM _meta WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .ok()
    };
    let source_commit = source_meta("git_head");
    let source_generation = source_meta("graph_generation");
    let source_built_at = source_meta("graph_published_at");
    let tag = resolve_tag(&store.db, repo_root, explicit_tag)?;
    let root = root_key(repo_root);
    // Forward-slash form to match the normalized paths stored in node rows;
    // strip Windows' extended-length `\\?\` prefix that canonicalize adds.
    let repo_root_prefix = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .display()
        .to_string()
        .replace("\\\\?\\", "")
        .replace('\\', "/");

    // One transaction covers prune, replacement, registration, AND the
    // cross-repo relation passes: commit the new graph or keep the old one
    // — never a half-written store, and never derived relations that
    // reference a store state that no longer exists.
    let tx = store.db.unchecked_transaction()?;
    prune_tag(&tx, &tag)?;

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
            .collect::<std::result::Result<Vec<_>, _>>()?;

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
            let source_file = if is_stub || source_file.is_empty() {
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
            .collect::<std::result::Result<Vec<_>, _>>()?;

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
                    if source_file.is_empty() { String::new() } else {
                        let rel = source_file.strip_prefix(&repo_root_prefix)
                            .unwrap_or(&source_file).trim_start_matches(['\\', '/']);
                        format!("{tag}/{rel}")
                    },
                    source_line,
                ],
            )?;
            edges_added += done as usize;
        }
    }

    // Registration is part of the same transaction: a root only claims its
    // tag when the graph actually landed.
    tx.execute(
        "INSERT OR REPLACE INTO global_roots (root, tag) VALUES (?1, ?2)",
        rusqlite::params![root, tag],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO global_snapshots (tag, source_commit, graph_generation, graph_built_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![tag, source_commit, source_generation, source_built_at],
    )?;

    // Relation reconciliation inside the same transaction (a store snapshot
    // must never be observable with the new nodes but stale same_type_as /
    // cross-repo-call edges), and the publication generation advances with
    // the store so snapshot caches invalidate.
    let same_type_edges = add_same_type_edges(&tx)?;
    let cross_repo_call_edges = resolve_cross_repo_calls(&tx)?;
    bump_generation(&tx)?;
    tx.commit()?;

    let mut result = GlobalAddResult {
        tag,
        nodes_added: 0,
        edges_added,
        same_type_edges,
        cross_repo_call_edges,
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
    pub root: String,
    pub source_commit: Option<String>,
    pub graph_generation: Option<String>,
    pub graph_built_at: Option<String>,
    pub state: String,
}

pub fn global_list(store: &GlobalStore) -> Result<Vec<GlobalListEntry>> {
    // Metadata is deliberately optional for unregistered graph nodes: unknown
    // provenance is shown explicitly rather than claimed to be fresh.
    let has_snapshots = store.db.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='global_snapshots'",
        [],
        |r| r.get::<_, i64>(0),
    )? > 0;
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
    let mut entries = Vec::new();
    for row in rows {
        let (tag, nodes, edges) = row?;
        let root: String = store
            .db
            .query_row("SELECT root FROM global_roots WHERE tag=?1", [&tag], |r| {
                r.get(0)
            })
            .unwrap_or_default();
        let snapshot: Option<(Option<String>, Option<String>, Option<String>)> = if has_snapshots {
            store.db.query_row("SELECT source_commit, graph_generation, graph_built_at FROM global_snapshots WHERE tag=?1", [&tag], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).ok()
        } else {
            None
        };
        let (source_commit, graph_generation, graph_built_at) = snapshot.unwrap_or_default();
        let state = if root.is_empty() || graph_generation.is_none() {
            "unknown"
        } else {
            match Connection::open_with_flags(
                Path::new(&root).join(".astria/db.sqlite"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            ) {
                Err(_) => "unavailable",
                Ok(db) => {
                    let current: Option<String> = db
                        .query_row(
                            "SELECT value FROM _meta WHERE key='graph_generation'",
                            [],
                            |r| r.get(0),
                        )
                        .ok();
                    let coverage = crate::verify_source_commit(root.clone()).ok();
                    if current != graph_generation
                        || coverage
                            .as_ref()
                            .is_some_and(|c| c.covers_head == Some(false))
                    {
                        "stale"
                    } else if coverage
                        .as_ref()
                        .is_some_and(|c| c.covers_head == Some(true))
                    {
                        "current"
                    } else {
                        "unknown"
                    }
                }
            }
        }
        .to_string();
        entries.push(GlobalListEntry {
            tag,
            nodes,
            edges,
            root,
            source_commit,
            graph_generation,
            graph_built_at,
            state,
        });
    }
    Ok(entries)
}

/// `same_type_as` edges: same-label type declarations (non-function shapes)
/// across different repos. Pairwise; skips existing edges.
fn add_same_type_edges(db: &Connection) -> Result<usize> {
    // Re-derive: retract the pass's own prior edges first.
    db.execute(
        "DELETE FROM edges WHERE context = 'global' AND relation = 'same_type_as'",
        [],
    )?;
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    {
        let mut stmt = db.prepare(
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
                if let Some(repo) = repo_of(db, id)? {
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
                let (Some(ra), Some(rb)) = (repo_of(db, &ids[i])?, repo_of(db, &ids[j])?) else {
                    continue;
                };
                if ra == rb {
                    continue;
                }
                let done = db.execute(
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
fn resolve_cross_repo_calls(db: &Connection) -> Result<usize> {
    db.execute(
        "DELETE FROM edges WHERE context = 'global' AND relation = 'calls'",
        [],
    )?;
    // Candidates: function-shaped, sourced (repo-tagged) nodes.
    let mut candidates: HashMap<String, Vec<(String, String)>> = HashMap::new(); // bare name -> [(id, repo)]
    {
        let mut stmt = db.prepare(
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
        let mut stmt = db.prepare(
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

        for (source, stub_target, stub_label, source_file) in rows {
            if stub_target.starts_with("receiver::") {
                continue;
            }
            let caller_repo = match repo_of(db, &source)? {
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
            let done = db.execute(
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
    let _writer = astria_core::writer_lock::WriterLock::acquire_in(
        store.path.parent().unwrap_or_else(|| Path::new(".")),
    )?;
    let tag = normalize_tag(tag);
    // Removal is one transaction: prune, registry delete, relation
    // re-derivation (a call that just became unambiguous may resolve now),
    // and the generation bump — the store never shows a half-removed state.
    ensure_roots_table(&store.db)?;
    let tx = store.db.unchecked_transaction()?;
    let removed = prune_tag(&tx, &tag)?;
    tx.execute(
        "DELETE FROM global_roots WHERE tag = ?1",
        rusqlite::params![tag],
    )?;
    tx.execute("DELETE FROM global_snapshots WHERE tag=?1", [&tag])?;
    add_same_type_edges(&tx)?;
    resolve_cross_repo_calls(&tx)?;
    bump_generation(&tx)?;
    tx.commit()?;
    Ok(removed)
}

/// Advance the store's publication generation. Snapshot caches key on this
/// stamp, so every writer must move it or readers keep serving the previous
/// graph.
pub fn bump_generation(db: &Connection) -> Result<()> {
    let stamp = crate::pipeline::generation_stamp(db);
    db.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('graph_generation', ?1)",
        [&stamp],
    )?;
    Ok(())
}

/// The store's publication generation, when any writer has stamped one.
pub fn generation_of_meta(db: &Connection) -> Option<String> {
    db.query_row(
        "SELECT value FROM _meta WHERE key = 'graph_generation'",
        [],
        |r| r.get::<_, String>(0),
    )
    .ok()
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
        let mut stmt = store
            .db
            .prepare("SELECT id FROM nodes WHERE LOWER(label) = LOWER(?1) ORDER BY id LIMIT 2")?;
        let mut rows = stmt.query_map(rusqlite::params![name], |r| r.get::<_, String>(0))?;
        if let Some(row) = rows.next() {
            let id = row?;
            if rows.next().is_some() {
                return Err(astria_core::AstriaError::Graph(format!(
                    "ambiguous node label {name}; use an exact node id"
                )));
            }
            return Ok(Some(id));
        }
        Ok(None)
    };

    let Some(start) = resolve(source)? else {
        return Ok(None);
    };
    let Some(goal) = resolve(target)? else {
        return Ok(None);
    };

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
    let mut participating = std::collections::HashSet::new();
    for id in &chain {
        if let Some(repo) = repo_of(&store.db, id)? {
            participating.insert(repo);
        }
    }
    let mut out = String::new();
    for snapshot in global_list(store)?
        .into_iter()
        .filter(|s| participating.contains(&s.tag))
    {
        out.push_str(&format!(
            "Snapshot {}: commit={} generation={} state={}\n",
            snapshot.tag,
            snapshot.source_commit.as_deref().unwrap_or("unknown"),
            snapshot.graph_generation.as_deref().unwrap_or("unknown"),
            snapshot.state
        ));
    }
    if chain.len() == 1 {
        out.push_str(&format!("{start} (same node)\n"));
    }
    for pair in chain.windows(2) {
        let (relation, confidence, outgoing): (String, String, bool) = store.db.query_row(
            "SELECT relation,confidence,source=?1 FROM edges WHERE (source=?1 AND target=?2) OR (source=?2 AND target=?1) ORDER BY confidence_score DESC LIMIT 1",
            rusqlite::params![pair[0],pair[1]], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let hop = if outgoing {
            format!("--{relation} [{confidence}]-->")
        } else {
            format!("<--{relation} [{confidence}]--")
        };
        out.push_str(&format!("{} {hop} {}\n", pair[0], pair[1]));
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::ids::normalize_id;

    fn seed_repo(path: &Path, crate_name: &str) -> Connection {
        let gdir = path.join(".astria");
        std::fs::create_dir_all(&gdir).unwrap();
        let db = astria_core::db::open_db(&gdir.join("db.sqlite")).unwrap();
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
    fn re_adding_a_registered_root_updates_its_own_tag() {
        // Re-adding the same root WITHOUT --as must update the existing tag,
        // not allocate `alpha-2` alongside it.
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("alpha");
        seed_repo(&repo_a, "alpha");
        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        global_add(&repo_a, Some("alpha"), &store).unwrap();
        let res = global_add(&repo_a, None, &store).unwrap();
        assert_eq!(res.tag, "alpha", "registered roots reuse their tag");
        let repos: i64 = store
            .db
            .query_row(
                "SELECT COUNT(DISTINCT repo) FROM nodes WHERE repo IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(repos, 1, "no duplicate tag for the same root");
    }

    #[test]
    fn explicit_tag_owned_by_another_root_is_rejected() {
        // Adding root B with --as alpha when alpha belongs to root A must
        // error; previously it pruned A's graph and replaced it.
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("alpha");
        let repo_b = dir.path().join("beta");
        seed_repo(&repo_a, "alpha");
        seed_repo(&repo_b, "beta");
        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        global_add(&repo_a, Some("alpha"), &store).unwrap();
        let err = global_add(&repo_b, Some("alpha"), &store).unwrap_err();
        assert!(err.to_string().contains("already in use"), "got: {err}");
        // alpha's graph survived.
        let alpha_nodes: i64 = store
            .db
            .query_row("SELECT COUNT(*) FROM nodes WHERE repo = 'alpha'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(alpha_nodes > 0, "rejected add must not prune the owner");
    }

    #[test]
    fn registered_root_cannot_be_retagged_without_removal() {
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("alpha");
        seed_repo(&repo_a, "alpha");
        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        global_add(&repo_a, Some("alpha"), &store).unwrap();
        let err = global_add(&repo_a, Some("renamed"), &store).unwrap_err();
        assert!(err.to_string().contains("already registered"), "got: {err}");
    }

    #[test]
    fn tag_collision_check_uses_normalized_names() {
        // `--as Foo-Bar` normalizes to `foo_bar`; if `foo_bar` is taken the
        // add must fail, not pass the raw-name check and prune the owner.
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("alpha");
        let repo_b = dir.path().join("beta");
        seed_repo(&repo_a, "alpha");
        seed_repo(&repo_b, "beta");
        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        global_add(&repo_a, Some("foo_bar"), &store).unwrap();
        let err = global_add(&repo_b, Some("Foo-Bar"), &store).unwrap_err();
        assert!(err.to_string().contains("already in use"), "got: {err}");
        let foo_bar_nodes: i64 = store
            .db
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE repo = 'foo_bar'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            foo_bar_nodes > 0,
            "converging name must not prune the owner"
        );
    }

    #[test]
    fn new_root_without_as_allocates_a_free_tag() {
        // F13 regression: the candidate loop used to return an error before
        // ever checking the candidates it built.
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("alpha");
        seed_repo(&repo_a, "alpha");
        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        let res = global_add(&repo_a, None, &store).unwrap();
        assert_eq!(res.tag, "alpha");
    }

    #[test]
    fn untagged_collisions_walk_the_candidate_ladder() {
        // Two different roots named "alpha": the second must get
        // <parent>_<name> or alpha-2, not an error.
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("one").join("alpha");
        let repo_b = dir.path().join("two").join("alpha");
        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();
        seed_repo(&repo_a, "alpha");
        seed_repo(&repo_b, "alpha");
        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        let first = global_add(&repo_a, None, &store).unwrap();
        assert_eq!(first.tag, "alpha");
        let second = global_add(&repo_b, None, &store).unwrap();
        assert!(
            second.tag == "two_alpha" || second.tag.starts_with("alpha-"),
            "unexpected tag {}",
            second.tag
        );
    }

    #[test]
    fn global_add_stamps_a_publication_generation() {
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("alpha");
        seed_repo(&repo_a, "alpha");
        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        assert_eq!(super::generation_of_meta(&store.db), None);
        global_add(&repo_a, Some("alpha"), &store).unwrap();
        assert!(super::generation_of_meta(&store.db).is_some());
    }

    #[test]
    fn remove_frees_the_tag_for_a_new_root() {
        let dir = tempfile::tempdir().unwrap();
        let repo_a = dir.path().join("alpha");
        let repo_b = dir.path().join("beta");
        seed_repo(&repo_a, "alpha");
        seed_repo(&repo_b, "beta");
        let store_dir = tempfile::tempdir().unwrap();
        let store = open_global_store_at(&store_dir.path().join("global.db")).unwrap();
        global_add(&repo_a, Some("alpha"), &store).unwrap();
        global_remove(&store, "alpha").unwrap();
        // The tag is claimable again after removal.
        global_add(&repo_b, Some("alpha"), &store).unwrap();
        let repos: Vec<String> = store
            .db
            .prepare("SELECT DISTINCT repo FROM nodes WHERE repo IS NOT NULL")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(repos, vec!["alpha".to_string()]);
    }

    #[test]
    fn cross_repo_call_resolves_single_candidate() {
        let dir = tempfile::tempdir().unwrap();
        // Repo A defines a bare function handler() and a STUB call target send_x().
        let repo_a = dir.path().join("caller");
        let gdir_a = repo_a.join(".astria");
        std::fs::create_dir_all(&gdir_a).unwrap();
        let db_a = astria_core::db::open_db(&gdir_a.join("db.sqlite")).unwrap();
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
        let gdir_b = repo_b.join(".astria");
        std::fs::create_dir_all(&gdir_b).unwrap();
        let db_b = astria_core::db::open_db(&gdir_b.join("db.sqlite")).unwrap();
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
        let gdir_a = repo_a.join(".astria");
        std::fs::create_dir_all(&gdir_a).unwrap();
        let db_a = astria_core::db::open_db(&gdir_a.join("db.sqlite")).unwrap();
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
            let gdir = repo.join(".astria");
            std::fs::create_dir_all(&gdir).unwrap();
            let db = astria_core::db::open_db(&gdir.join("db.sqlite")).unwrap();
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
