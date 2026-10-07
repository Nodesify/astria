// merge: combine two graph databases into one fresh output graph.
//
// Identity (F14): node ids are repository-relative, so the same id in two
// unrelated roots names two different entities. By default each input's
// sourced nodes are namespaced `<tag>::` — the same policy the global store
// uses — and every edge endpoint is remapped through the same mapping;
// stub/external nodes keep their ids and unify by label across inputs.
// `--same-repo` merges assume one repository: no namespacing, and a
// conflicting definition under the same id is an error, never a silent
// first-wins.
//
// Publication (F15): the output is built in a staging database inside one
// transaction, deduplicated (repeating an identical merge yields the same
// graph), and renamed into place — a failed merge never leaves a partial
// output behind.
//
// Artifacts (F16): merged nodes keep signatures, merged edges keep context
// and line anchors, and the merge publishes the same artifacts the pipeline
// does (`graph_report.md`, `graph.json`) plus merge provenance in `_meta`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

#[derive(Debug)]
pub struct MergeResult {
    pub nodes_added: i64,
    pub edges_added: i64,
    pub communities: usize,
    pub report: String,
}

pub struct DiffResult {
    pub nodes_added: i64,
    pub nodes_removed: i64,
    pub edges_added: i64,
    pub edges_removed: i64,
    pub added_node_labels: Vec<String>,
    pub removed_node_labels: Vec<String>,
}

pub fn merge_graphs(
    root_a: &Path,
    root_b: &Path,
    out_root: &Path,
) -> astria_core::Result<MergeResult> {
    merge_graphs_with_policy(root_a, root_b, out_root, false)
}

/// Logical edge identity: the full current-schema row. `confidence_score`
/// rides as raw f64 bits so the set deduplicates rows exactly (f64 has no
/// Hash/Eq).
#[derive(Clone, PartialEq, Eq, Hash)]
struct LogicalEdge {
    source: String,
    target: String,
    relation: String,
    confidence: String,
    score_bits: Option<u64>,
    source_file: String,
    source_line: Option<i64>,
    context: Option<String>,
}

pub fn merge_graphs_with_policy(
    root_a: &Path,
    root_b: &Path,
    out_root: &Path,
    same_repo: bool,
) -> astria_core::Result<MergeResult> {
    let _writer = astria_core::writer_lock::WriterLock::acquire(out_root)?;
    let db_a = crate::pipeline::load_graph_db(root_a)?;
    let db_b = crate::pipeline::load_graph_db(root_b)?;

    let canon = |p: &Path| -> PathBuf { p.canonicalize().unwrap_or_else(|_| p.to_path_buf()) };
    let (ca, cb, cout) = (canon(root_a), canon(root_b), canon(out_root));
    if ca == cb {
        return Err(astria_core::AstriaError::Graph(
            "cannot merge a graph with itself — use diff to compare two builds".into(),
        ));
    }
    if cout == ca || cout == cb {
        return Err(astria_core::AstriaError::Graph(
            "merge output must not alias an input graph".into(),
        ));
    }

    let (tag_a, tag_b) = if same_repo {
        (String::new(), String::new())
    } else {
        root_tags(root_a, root_b)
    };

    let out_astria = out_root.join(".astria");
    std::fs::create_dir_all(&out_astria)?;
    let final_db = out_astria.join("db.sqlite");
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let staging = out_astria.join(format!("db.sqlite.merging-{nonce}"));
    let db = astria_core::db::open_db(&staging)?;

    let tx = db.unchecked_transaction()?;

    // Stub/external nodes unify by label across both inputs; sourced nodes
    // are namespaced per input (or shared verbatim in --same-repo mode).
    let mut external_by_label: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut nodes_added = 0i64;
    let map_a = insert_nodes(&tx, &db_a, &tag_a, &mut external_by_label, &mut nodes_added)?;
    let map_b = insert_nodes(&tx, &db_b, &tag_b, &mut external_by_label, &mut nodes_added)?;

    // Logical-edge deduplication across both inputs: identical merges yield
    // identical graphs instead of accumulating duplicate rows.
    let mut edges_added = 0i64;
    let mut logical_edges: HashSet<LogicalEdge> = HashSet::new();
    collect_edges(&db_a, &map_a, &mut logical_edges)?;
    collect_edges(&db_b, &map_b, &mut logical_edges)?;
    for edge in &logical_edges {
        tx.execute(
            "INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file, source_line, context)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                edge.source,
                edge.target,
                edge.relation,
                edge.confidence,
                edge.score_bits.map(f64::from_bits),
                edge.source_file,
                edge.source_line,
                edge.context
            ],
        )?;
        edges_added += 1;
    }

    // Merge provenance alongside the standard publication stamps.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    tx.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('graph_published_at', ?1)",
        [&now],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('pipeline_version', ?1)",
        [env!("CARGO_PKG_VERSION")],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('merge_sources', ?1)",
        [format!(
            "{}{}|{}{}",
            tag_a,
            root_a.display(),
            tag_b,
            root_b.display()
        )],
    )?;
    // The merged database is born with a publication generation: snapshot
    // caches key on it, so a published merge never reads as its input's
    // stale generation. Captured here so the artifacts can carry the same
    // stamp after the swap below.
    let stamp = crate::pipeline::generation_stamp(&tx);
    tx.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('graph_generation', ?1)",
        [&stamp],
    )?;

    tx.commit()?;

    let cluster_result = astria_cluster::cluster(&db)?;
    let analysis = astria_analyze::analyze(&db)?;
    let report = astria_report::generate_report(&db, &analysis)?;

    // Artifacts are STAGED under `.new` names and only become visible after
    // the database swap succeeds: a failure anywhere above leaves both the
    // previous database and the previous artifacts in place.
    let json_new = out_astria.join(format!("graph.json.new-{nonce}"));
    crate::pipeline::export_json(&db, &json_new)?;
    let report_new = out_astria.join(format!("graph_report.md.new-{nonce}"));
    write_atomic(
        &report_new,
        format!("{report}\n---\n\ngeneration: {stamp}\n").as_bytes(),
    )?;

    // Publish the database by rename. The connection must be closed first
    // (Windows keeps open files locked); checkpointing folds the WAL in so
    // the single renamed file is the complete database. On failure the
    // previous database is restored — the expected path always exists.
    let _ = db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    drop(db);
    publish_db(&staging, &final_db)?;

    // Database published; flip the staged artifacts into place and record
    // the generation sidecar.
    std::fs::rename(&json_new, out_astria.join("graph.json"))?;
    std::fs::rename(&report_new, out_astria.join("graph_report.md"))?;
    write_atomic(&out_astria.join("generation.txt"), stamp.as_bytes())?;

    Ok(MergeResult {
        nodes_added,
        edges_added,
        communities: cluster_result.communities.len(),
        report,
    })
}

/// Distinct tags for the two inputs: the root directory name each, widened
/// with the parent when both names match, so `a/proj` and `b/proj` never
/// share a namespace.
fn root_tags(root_a: &Path, root_b: &Path) -> (String, String) {
    let name = |p: &Path| {
        p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo")
            .to_string()
    };
    let parent = |p: &Path| {
        p.parent()
            .and_then(|q| q.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("dir")
            .to_string()
    };
    let (mut ta, mut tb) = (name(root_a), name(root_b));
    if ta == tb {
        ta = format!("{}_{}", parent(root_a), ta);
        tb = format!("{}_{}", parent(root_b), tb);
    }
    if ta == tb {
        ta.push_str("_a");
        tb.push_str("_b");
    }
    (
        astria_core::ids::normalize_id(&ta),
        astria_core::ids::normalize_id(&tb),
    )
}

/// One graph node row, carried with the columns the current schema defines.
type NodeRow = (
    String,         // id
    String,         // label
    String,         // file_type
    String,         // source_file
    Option<i64>,    // source_line
    Option<String>, // docstring
    Option<String>, // signature
    Option<i64>,    // community
);

const NODE_QUERY: &str = "SELECT id, label, file_type, source_file, source_line, docstring, signature, community FROM nodes";

fn load_nodes(source: &Connection) -> astria_core::Result<Vec<NodeRow>> {
    let mut stmt = source.prepare(NODE_QUERY)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
        ))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Insert one input's nodes into the merged graph. Returns the
/// local→global id map used to remap edge endpoints. In `--same-repo`
/// mode (empty tag) a conflicting definition under an already-inserted id
/// is an error, not a silent first-wins.
fn insert_nodes(
    tx: &rusqlite::Transaction<'_>,
    source: &Connection,
    tag: &str,
    external_by_label: &mut std::collections::HashMap<String, String>,
    count: &mut i64,
) -> astria_core::Result<std::collections::HashMap<String, String>> {
    let mut local_to_global = std::collections::HashMap::new();
    for (id, label, file_type, source_file, source_line, docstring, signature, community) in
        load_nodes(source)?
    {
        let is_stub = file_type == "stub";
        let (global_id, final_label) = if is_stub {
            match external_by_label.get(&label.to_lowercase()) {
                Some(existing) => (existing.clone(), label),
                None => {
                    external_by_label.insert(label.to_lowercase(), id.clone());
                    (id.clone(), label.clone())
                }
            }
        } else if tag.is_empty() {
            // Same-repository merge: the id is already globally meaningful.
            // A prior, DIFFERENT definition under the same id is a conflict.
            let prior: Option<(String, String, Option<String>, Option<String>)> = tx
                .query_row(
                    "SELECT label, source_file, docstring, signature FROM nodes WHERE id = ?1",
                    rusqlite::params![id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .ok();
            if let Some((p_label, p_source, p_doc, p_sig)) = prior {
                let identical = (
                    p_label.clone(),
                    p_source.clone(),
                    p_doc.clone(),
                    p_sig.clone(),
                ) == (
                    label.clone(),
                    source_file.clone(),
                    docstring.clone(),
                    signature.clone(),
                );
                if !identical {
                    return Err(astria_core::AstriaError::Graph(format!(
                        "same-repo merge conflict on node '{id}': definitions differ \
                         ({p_label} in {p_source} vs {label} in {source_file}); \
                         resolve the divergence or merge with repository namespacing"
                    )));
                }
                local_to_global.insert(id.clone(), id);
                continue;
            }
            (id.clone(), label.clone())
        } else {
            (format!("{tag}::{id}"), label.clone())
        };
        // Stubs dedup by id (label unification already points repeats at the
        // first); sourced inserts are unique by construction and must fail
        // loudly rather than silently drop a definition.
        let sql = if is_stub {
            "INSERT OR IGNORE INTO nodes (id, label, file_type, source_file, source_line, docstring, signature, community)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        } else {
            "INSERT INTO nodes (id, label, file_type, source_file, source_line, docstring, signature, community)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        };
        let done = tx.execute(
            sql,
            rusqlite::params![
                global_id,
                final_label,
                file_type,
                source_file,
                source_line,
                docstring,
                signature,
                community
            ],
        )?;
        if done > 0 {
            *count += 1;
        }
        local_to_global.insert(id, global_id);
    }
    Ok(local_to_global)
}

/// Read one input's edges, remap endpoints through the node map, and fold
/// them into the logical-edge set (deduplicating identical rows).
fn collect_edges(
    source: &Connection,
    map: &std::collections::HashMap<String, String>,
    logical: &mut HashSet<LogicalEdge>,
) -> astria_core::Result<()> {
    let mut stmt = source.prepare(
        "SELECT source, target, relation, confidence, confidence_score, source_file, source_line, context FROM edges",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<f64>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;
    for row in rows {
        let (src, tgt, relation, confidence, score, source_file, line, context) = row?;
        let (Some(gs), Some(gt)) = (map.get(&src).cloned(), map.get(&tgt).cloned()) else {
            continue; // endpoint absent from the source's node set
        };
        if gs == gt {
            continue; // remapping collapsed a self-loop
        }
        logical.insert(LogicalEdge {
            source: gs,
            target: gt,
            relation,
            confidence,
            score_bits: score.map(|f| f.to_bits()),
            source_file,
            source_line: line,
            context,
        });
    }
    Ok(())
}

/// Swap the staged database into its final name. Windows cannot rename over
/// an existing file, so the previous output moves aside first — and if the
/// staging rename then fails, it is moved BACK: the expected database path
/// exists again on every exit, success or failure.
fn publish_db(staging: &Path, final_db: &Path) -> astria_core::Result<()> {
    // Atomic replacement keeps the previous database visible until the swap.
    tempfile::TempPath::try_from_path(staging)?
        .persist(final_db)
        .map_err(|e| e.error)?;
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(PathBuf::from(format!("{}{suffix}", staging.display())));
    }
    Ok(())
}

/// Write a file whole via a temp sibling + rename, so a concurrent reader
/// never observes a torn artifact.
fn write_atomic(path: &Path, bytes: &[u8]) -> astria_core::Result<()> {
    astria_core::writer_lock::write_atomic(path, bytes)
}

pub fn diff_graphs(root_a: &Path, root_b: &Path) -> astria_core::Result<DiffResult> {
    let db_a = crate::pipeline::load_graph_db(root_a)?;
    let db_b = crate::pipeline::load_graph_db(root_b)?;

    let ids_a: HashSet<String> = collect_ids(&db_a, "SELECT id FROM nodes")?;
    let ids_b: HashSet<String> = collect_ids(&db_b, "SELECT id FROM nodes")?;

    let added_ids: Vec<String> = ids_b.difference(&ids_a).cloned().collect();
    let removed_ids: Vec<String> = ids_a.difference(&ids_b).cloned().collect();

    let edges_a: HashSet<(String, String, String)> = collect_edges_for_diff(&db_a)?;
    let edges_b: HashSet<(String, String, String)> = collect_edges_for_diff(&db_b)?;

    let added_edges = edges_b.difference(&edges_a).count() as i64;
    let removed_edges = edges_a.difference(&edges_b).count() as i64;

    let added_labels = query_labels(&db_b, &added_ids)?;
    let removed_labels = query_labels(&db_a, &removed_ids)?;

    Ok(DiffResult {
        nodes_added: added_ids.len() as i64,
        nodes_removed: removed_ids.len() as i64,
        edges_added: added_edges,
        edges_removed: removed_edges,
        added_node_labels: added_labels,
        removed_node_labels: removed_labels,
    })
}

// -- internal helpers --

fn collect_ids(db: &Connection, query: &str) -> astria_core::Result<HashSet<String>> {
    let mut stmt = db.prepare(query)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let set = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(set.into_iter().collect())
}

fn collect_edges_for_diff(
    db: &Connection,
) -> astria_core::Result<HashSet<(String, String, String)>> {
    let mut stmt = db.prepare("SELECT source, target, relation FROM edges")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let set = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(set.into_iter().collect())
}

fn query_labels(db: &Connection, ids: &[String]) -> astria_core::Result<Vec<String>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
    let q = format!(
        "SELECT label FROM nodes WHERE id IN ({})",
        placeholders.join(",")
    );
    let params: Vec<&dyn rusqlite::types::ToSql> = ids
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();
    let mut stmt = db.prepare(&q)?;
    let rows = stmt.query_map(params.as_slice(), |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_db(db: &Connection, nodes: &[(&str, &str, &str)], edges: &[(&str, &str, &str)]) {
        for &(id, label, sf) in nodes {
            db.execute(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES (?1, ?2, 'code', ?3)",
                rusqlite::params![id, label, sf],
            )
            .unwrap();
        }
        for &(src, tgt, rel) in edges {
            db.execute(
                "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES (?1, ?2, ?3, 'EXTRACTED', 'test.py')",
                rusqlite::params![src, tgt, rel],
            ).unwrap();
        }
    }

    fn make_graph_dir(
        nodes: &[(&str, &str, &str)],
        edges: &[(&str, &str, &str)],
    ) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let gf = dir.path().join(".astria");
        std::fs::create_dir_all(&gf).unwrap();
        let db_path = gf.join("db.sqlite");
        let db = astria_core::db::open_db(&db_path).unwrap();
        seed_db(&db, nodes, edges);
        dir
    }

    fn make_graph_dir_named(
        name: &str,
        nodes: &[(&str, &str, &str)],
        edges: &[(&str, &str, &str)],
    ) -> tempfile::TempDir {
        // The parent dir name feeds tag widening; keep each repo in its own
        // named directory so tags are deterministic.
        let outer = tempfile::tempdir().unwrap();
        let repo = outer.path().join(name);
        let gf = repo.join(".astria");
        std::fs::create_dir_all(&gf).unwrap();
        let db = astria_core::db::open_db(&gf.join("db.sqlite")).unwrap();
        seed_db(&db, nodes, edges);
        // Leak the outer tempdir's lifetime by re-rooting it: tempdir cleans
        // up on drop, so hand out a TempDir whose path is the repo itself.
        drop(db);
        // Safety: move the repo under a new tempdir that we return.
        let keep = tempfile::tempdir().unwrap();
        let dst = keep.path().join(name);
        std::fs::rename(&repo, &dst).unwrap();
        let _ = outer;
        keep
    }

    fn repo_path(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    #[test]
    fn merge_two_disjoint_graphs() {
        let dir_a = make_graph_dir(
            &[("a::Foo", "Foo", "a.py"), ("a::Bar", "Bar", "a.py")],
            &[("a::Foo", "a::Bar", "calls")],
        );
        let dir_b = make_graph_dir(
            &[("b::Baz", "Baz", "b.py"), ("b::Qux", "Qux", "b.py")],
            &[("b::Baz", "b::Qux", "calls")],
        );
        let out = tempfile::tempdir().unwrap();

        let result = merge_graphs(dir_a.path(), dir_b.path(), out.path()).unwrap();
        assert_eq!(result.nodes_added, 4);
        assert_eq!(result.edges_added, 2);
        // F16: the advertised artifacts exist.
        assert!(out.path().join(".astria/graph.json").exists());
        assert!(out.path().join(".astria/graph_report.md").exists());
    }

    #[test]
    fn cross_repo_same_ids_stay_distinct_and_connected() {
        // Two unrelated roots both defining `src_index_ts::main` must keep
        // both definitions (namespaced) — previously the first silently won
        // and the second repo's edges attached to it.
        let nodes = &[("src_index_ts::main", "main()", "src/index.ts")];
        let dir_a = make_graph_dir_named("alpha", nodes, &[]);
        let dir_b = make_graph_dir_named("beta", nodes, &[]);
        let out = tempfile::tempdir().unwrap();

        merge_graphs(
            &repo_path(&dir_a, "alpha"),
            &repo_path(&dir_b, "beta"),
            out.path(),
        )
        .unwrap();
        let db = astria_core::db::open_db(&out.path().join(".astria/db.sqlite")).unwrap();
        let both: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE id LIKE '%src_index_ts::main'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(both, 2, "each repo keeps its own definition");
    }

    #[test]
    fn same_repo_merge_conflicts_fail_loud() {
        let nodes = &[("src_lib::helper", "helper()", "src/lib.rs")];
        let dir_a = make_graph_dir_named("proj", nodes, &[]);
        let conflicting = &[("src_lib::helper", "helper_v2()", "src/lib.rs")];
        let dir_b = make_graph_dir_named("proj", conflicting, &[]);
        let out = tempfile::tempdir().unwrap();

        let err = merge_graphs_with_policy(
            &repo_path(&dir_a, "proj"),
            &repo_path(&dir_b, "proj"),
            out.path(),
            true,
        )
        .unwrap_err();
        assert!(err.to_string().contains("conflict"), "got: {err}");

        // Identical definitions merge cleanly in same-repo mode.
        let dir_c = make_graph_dir_named("proj", nodes, &[]);
        let ok = merge_graphs_with_policy(
            &repo_path(&dir_a, "proj"),
            &repo_path(&dir_c, "proj"),
            out.path(),
            true,
        )
        .unwrap();
        assert_eq!(ok.nodes_added, 1);
    }

    #[test]
    fn repeat_merge_is_idempotent_not_accumulating() {
        let nodes_a = &[("a::Foo", "Foo", "a.py"), ("ext", "ext", "deps")];
        let nodes_b = &[("b::Bar", "Bar", "b.py")];
        let dir_a = make_graph_dir(nodes_a, &[("a::Foo", "ext", "calls")]);
        let dir_b = make_graph_dir(nodes_b, &[]);
        let out = tempfile::tempdir().unwrap();

        merge_graphs(dir_a.path(), dir_b.path(), out.path()).unwrap();
        let first_edges: i64 = {
            let db = astria_core::db::open_db(&out.path().join(".astria/db.sqlite")).unwrap();
            db.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
                .unwrap()
        };
        // Rebuilding the same merge must land on the same graph, not append.
        merge_graphs(dir_a.path(), dir_b.path(), out.path()).unwrap();
        let second_edges: i64 = {
            let db = astria_core::db::open_db(&out.path().join(".astria/db.sqlite")).unwrap();
            db.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(
            first_edges, second_edges,
            "repeated merge must not duplicate edges"
        );
    }

    #[test]
    fn output_aliasing_inputs_is_rejected() {
        let dir_a = make_graph_dir(&[("a::Foo", "Foo", "a.py")], &[]);
        let dir_b = make_graph_dir(&[("b::Bar", "Bar", "b.py")], &[]);
        let err = merge_graphs(dir_a.path(), dir_b.path(), dir_a.path()).unwrap_err();
        assert!(err.to_string().contains("alias"), "got: {err}");
        let err = merge_graphs(dir_a.path(), dir_b.path(), dir_b.path()).unwrap_err();
        assert!(err.to_string().contains("alias"), "got: {err}");
    }

    #[test]
    fn merging_a_graph_with_itself_is_rejected() {
        let dir_a = make_graph_dir(&[("a::Foo", "Foo", "a.py")], &[]);
        let out = tempfile::tempdir().unwrap();
        let err = merge_graphs(dir_a.path(), dir_a.path(), out.path()).unwrap_err();
        assert!(err.to_string().contains("itself"), "got: {err}");
    }

    #[test]
    fn signatures_and_edge_context_survive() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sig");
        let gf = repo.join(".astria");
        std::fs::create_dir_all(&gf).unwrap();
        let db = astria_core::db::open_db(&gf.join("db.sqlite")).unwrap();
        db.execute(
            "INSERT INTO nodes (id, label, file_type, source_file, signature) VALUES ('n1', 'f()', 'code', 'a.rs', 'pub fn f() -> u32')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('n1x', 'g()', 'code', 'a.rs')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO edges (source, target, relation, confidence, source_file, context) VALUES ('n1', 'n1x', 'calls', 'EXTRACTED', 'a.rs', 'deep')",
            [],
        )
        .unwrap();
        drop(db);

        let other = make_graph_dir_named("other", &[("o::x", "x", "o.rs")], &[]);
        let out = tempfile::tempdir().unwrap();
        merge_graphs(&repo, &repo_path(&other, "other"), out.path()).unwrap();

        let merged = astria_core::db::open_db(&out.path().join(".astria/db.sqlite")).unwrap();
        let sig: String = merged
            .query_row(
                "SELECT signature FROM nodes WHERE id LIKE '%::n1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sig, "pub fn f() -> u32");
        let context: Option<String> = merged
            .query_row(
                "SELECT context FROM edges WHERE relation = 'calls'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(context.as_deref(), Some("deep"));
    }

    #[test]
    fn diff_identical_graphs() {
        let nodes = &[("n1", "Alpha", "f.py"), ("n2", "Beta", "f.py")];
        let edges = &[("n1", "n2", "calls")];
        let dir_a = make_graph_dir(nodes, edges);
        let dir_b = make_graph_dir(nodes, edges);

        let result = diff_graphs(dir_a.path(), dir_b.path()).unwrap();
        assert_eq!(result.nodes_added, 0);
        assert_eq!(result.nodes_removed, 0);
        assert_eq!(result.edges_added, 0);
        assert_eq!(result.edges_removed, 0);
    }

    #[test]
    fn diff_graph_with_added_nodes() {
        let dir_a = make_graph_dir(&[("n1", "Alpha", "f.py")], &[]);
        let dir_b = make_graph_dir(&[("n1", "Alpha", "f.py"), ("n2", "Beta", "g.py")], &[]);

        let result = diff_graphs(dir_a.path(), dir_b.path()).unwrap();
        assert_eq!(result.nodes_added, 1);
        assert_eq!(result.nodes_removed, 0);
        assert!(result.added_node_labels.contains(&"Beta".to_string()));
    }

    #[test]
    fn diff_graph_with_removed_nodes() {
        let dir_a = make_graph_dir(&[("n1", "Alpha", "f.py"), ("n2", "Beta", "g.py")], &[]);
        let dir_b = make_graph_dir(&[("n1", "Alpha", "f.py")], &[]);

        let result = diff_graphs(dir_a.path(), dir_b.path()).unwrap();
        assert_eq!(result.nodes_added, 0);
        assert_eq!(result.nodes_removed, 1);
        assert!(result.removed_node_labels.contains(&"Beta".to_string()));
    }
}
