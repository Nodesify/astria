//! Imported indexes own their complete facts independently of source citations.
use std::collections::BTreeSet;
use std::path::PathBuf;

use astria_core::Result;
use astria_extract::Extraction;
use rusqlite::{params, Connection};

fn schema(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS external_node_facts (
        owner TEXT NOT NULL, node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
        label TEXT NOT NULL, node_type TEXT NOT NULL, source_file TEXT NOT NULL,
        source_line INTEGER, docstring TEXT, signature TEXT,
        PRIMARY KEY(owner, node_id)
    );
    CREATE INDEX IF NOT EXISTS external_facts_node ON external_node_facts(node_id);
    CREATE INDEX IF NOT EXISTS external_facts_source ON external_node_facts(source_file);
    CREATE TABLE IF NOT EXISTS external_index_status (
        owner TEXT PRIMARY KEY, stale INTEGER NOT NULL, reason TEXT
    );",
    )?;
    Ok(())
}

fn previous_nodes(owner: &str, db: &Connection) -> Result<BTreeSet<String>> {
    let mut stmt = db.prepare("SELECT node_id FROM external_node_facts WHERE owner=?1")?;
    let rows = stmt.query_map([owner], |row| row.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn remove_owner(owner: &str, db: &Connection) -> Result<BTreeSet<String>> {
    let nodes = previous_nodes(owner, db)?;
    db.execute("DELETE FROM edges WHERE context=?1", [owner])?;
    db.execute("DELETE FROM external_node_facts WHERE owner=?1", [owner])?;
    Ok(nodes)
}

/// Recompute canonical symbols from their surviving owners. Prefer a located
/// definition over a reference, then richer metadata, then stable owner order.
fn refresh(nodes: &BTreeSet<String>, db: &Connection) -> Result<()> {
    for id in nodes {
        let surviving: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM external_node_facts WHERE node_id=?1)",
            [id],
            |row| row.get(0),
        )?;
        if !surviving {
            db.execute("DELETE FROM edges WHERE source=?1 OR target=?1", [id])?;
            db.execute("DELETE FROM nodes WHERE id=?1", [id])?;
            continue;
        }
        // A changed canonical text invalidates its embedding in this same
        // publication transaction. The located owner's metadata stays together.
        db.execute("DELETE FROM node_embeddings WHERE node_id=?1", [id])?;
        db.execute("UPDATE nodes SET (label,file_type,source_file,source_line,docstring,signature) = (
            SELECT label,
                CASE WHEN node_type IN ('reference','stub','test') THEN node_type ELSE 'code' END,
                source_file,source_line,docstring,signature
            FROM external_node_facts WHERE node_id=?1
            ORDER BY
                CASE WHEN source_file <> '' AND node_type NOT IN ('reference','stub') AND source_line IS NOT NULL THEN 3
                     WHEN source_file <> '' AND node_type NOT IN ('reference','stub') THEN 2
                     WHEN node_type NOT IN ('reference','stub') THEN 1 ELSE 0 END DESC,
                (length(COALESCE(docstring,'')) + length(COALESCE(signature,''))) DESC,
                owner ASC
            LIMIT 1
        ) WHERE id=?1", [id])?;
    }
    Ok(())
}

fn stamp_stale(db: &Connection) -> Result<()> {
    let owners: Vec<String> = {
        let mut stmt =
            db.prepare("SELECT owner FROM external_index_status WHERE stale=1 ORDER BY owner")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let value = serde_json::to_string(&owners)
        .map_err(|e| astria_core::AstriaError::Graph(e.to_string()))?;
    db.execute(
        "INSERT OR REPLACE INTO _meta(key,value) VALUES('external_indexes_stale',?1)",
        [value],
    )?;
    Ok(())
}

/// Called inside source publication's transaction. Compiler index overlays are
/// invalidated as a whole when any indexed document changes, including documents
/// that contain only references (their citation is on the owned edge).
pub fn invalidate_sources(paths: &[PathBuf], db: &Connection) -> Result<usize> {
    schema(db)?;
    let mut owners = BTreeSet::<String>::new();
    for path in paths {
        let key = astria_paths::normalize(path);
        let mut stmt = db.prepare("SELECT owner FROM external_node_facts WHERE source_file=?1
            UNION SELECT context FROM edges WHERE source_file=?1 AND context LIKE 'external-index:%'")?;
        let rows = stmt.query_map([key], |row| row.get(0))?;
        owners.extend(rows.collect::<std::result::Result<Vec<String>, _>>()?);
    }
    let mut nodes = BTreeSet::new();
    for owner in &owners {
        nodes.extend(remove_owner(owner, db)?);
        db.execute("INSERT INTO external_index_status(owner,stale,reason) VALUES(?1,1,'indexed source changed or was removed')
            ON CONFLICT(owner) DO UPDATE SET stale=1,reason=excluded.reason", [owner])?;
    }
    refresh(&nodes, db)?;
    stamp_stale(db)?;
    Ok(owners.len())
}

pub fn replace(extraction: &Extraction, db: &Connection) -> Result<super::BuildResult> {
    super::validate::assert_valid(std::slice::from_ref(extraction))?;
    let owner = format!(
        "external-index:{}",
        astria_paths::normalize(&extraction.file_path)
    );
    let tx = db.unchecked_transaction()?;
    schema(&tx)?;
    let mut touched = remove_owner(&owner, &tx)?;
    for node in &extraction.nodes {
        touched.insert(node.id.clone());
        tx.execute(
            "INSERT OR IGNORE INTO nodes(id,label,file_type,source_file) VALUES(?1,?1,'stub','')",
            [&node.id],
        )?;
        tx.execute("INSERT INTO external_node_facts(owner,node_id,label,node_type,source_file,source_line,docstring,signature)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![owner,node.id,astria_core::sanitize_label(&node.label),node.node_type,
                astria_paths::normalize(&node.source_file),node.source_line,
                node.docstring.as_deref().map(astria_core::sanitize_docstring),
                node.signature.as_deref().map(astria_core::sanitize_docstring)])?;
    }
    for edge in &extraction.edges {
        for id in [&edge.source, &edge.target] {
            touched.insert(id.clone());
            tx.execute("INSERT OR IGNORE INTO nodes(id,label,file_type,source_file) VALUES(?1,?1,'stub','')", [id])?;
            // Edge-only endpoints own a real stub fact, so removing another
            // owner's definition cannot erase this surviving owner's reference.
            tx.execute("INSERT OR IGNORE INTO external_node_facts(owner,node_id,label,node_type,source_file)
                VALUES(?1,?2,?2,'stub','')", params![owner,id])?;
        }
        tx.execute("INSERT INTO edges(source,target,relation,confidence,confidence_score,source_file,source_line,context)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![edge.source,edge.target,edge.relation,edge.confidence,
                edge.confidence_score,astria_paths::normalize(&edge.source_file),edge.source_line,owner])?;
    }
    refresh(&touched, &tx)?;
    tx.execute(
        "INSERT INTO external_index_status(owner,stale,reason) VALUES(?1,0,NULL)
        ON CONFLICT(owner) DO UPDATE SET stale=0,reason=NULL",
        [&owner],
    )?;
    stamp_stale(&tx)?;
    let generation = format!(
        "external:{}:{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    tx.execute(
        "INSERT OR REPLACE INTO _meta(key,value) VALUES('graph_generation',?1)",
        [&generation],
    )?;
    tx.commit()?;
    Ok(super::BuildResult {
        nodes_added: extraction.nodes.len(),
        edges_added: extraction.edges.len(),
        duplicates_merged: 0,
    })
}
