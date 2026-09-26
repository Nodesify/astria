use crate::error::Result;
use rusqlite::Connection;

const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS extraction_cache (
    file_path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    language TEXT NOT NULL,
    nodes TEXT NOT NULL,
    edges TEXT NOT NULL,
    extracted_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS file_manifest (
    file_path TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    file_type TEXT NOT NULL,
    language TEXT,
    last_seen_at TEXT NOT NULL,
    size_bytes INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS nodes (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    file_type TEXT NOT NULL,
    source_file TEXT NOT NULL,
    source_line INTEGER,
    docstring TEXT,
    community INTEGER,
    degree_centrality REAL
);
CREATE INDEX IF NOT EXISTS idx_nodes_file ON nodes(source_file);
CREATE INDEX IF NOT EXISTS idx_nodes_community ON nodes(community);

CREATE TABLE IF NOT EXISTS edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL REFERENCES nodes(id),
    target TEXT NOT NULL REFERENCES nodes(id),
    relation TEXT NOT NULL,
    confidence TEXT NOT NULL,
    confidence_score REAL,
    source_file TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);

CREATE TABLE IF NOT EXISTS pipeline_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL,
    files_processed INTEGER,
    nodes_added INTEGER,
    edges_added INTEGER
);

CREATE TABLE IF NOT EXISTS query_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    question TEXT NOT NULL,
    answer TEXT,
    path_taken TEXT,
    queried_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value TEXT);
INSERT OR IGNORE INTO _meta (key, value) VALUES ('schema_version', '1');
";

const SCHEMA_V2: &str = "
CREATE TABLE IF NOT EXISTS communities (
    id INTEGER PRIMARY KEY,
    label TEXT NOT NULL,
    cohesion REAL,
    size INTEGER NOT NULL DEFAULT 0
);
";

const SCHEMA_V5: &str = "
CREATE TABLE IF NOT EXISTS node_embeddings (
    node_id TEXT PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
    dim INTEGER NOT NULL,
    embedding BLOB NOT NULL,
    model TEXT NOT NULL,
    embedded_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_node_embeddings_model ON node_embeddings(model);
";

const SCHEMA_V6: &str = "
CREATE TABLE IF NOT EXISTS query_pairs (
    source TEXT NOT NULL,
    target TEXT NOT NULL,
    question TEXT NOT NULL,
    hits INTEGER NOT NULL DEFAULT 1,
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    PRIMARY KEY (source, target, question)
);
";

const SCHEMA_V7: &str = "
CREATE TABLE IF NOT EXISTS hyperedges (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    nodes TEXT NOT NULL,
    relation TEXT NOT NULL,
    confidence TEXT NOT NULL,
    confidence_score REAL,
    source_file TEXT NOT NULL DEFAULT ''
);
";

/// Run any pending schema migrations.
fn run_migrations(conn: &Connection) -> Result<()> {
    let version: i64 = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM _meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if version < 1 {
        conn.execute_batch(SCHEMA_V1)?;
    }
    if version < 2 {
        // v2: community labels + cohesion (hub-based, LLM-free)
        conn.execute_batch(SCHEMA_V2)?;
        conn.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', '2')",
            [],
        )?;
    }
    if version < 3 {
        // v3: node signatures (source text up to the body) for token-cheap
        // "what is this symbol" answers. Older graphs get NULL signatures
        // until the next full re-extraction.
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN signature TEXT;")?;
        conn.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', '3')",
            [],
        )?;
    }
    if version < 4 {
        // v4: edge provenance — the source line where an edge was extracted
        // — so query/explain output can anchor relationships to code.
        conn.execute_batch("ALTER TABLE edges ADD COLUMN source_line INTEGER;")?;
        conn.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', '4')",
            [],
        )?;
    }
    if version < 5 {
        // v5: local-embedding vectors for semantic similarity edges and
        // embedding-backed query recall (see astria-embed).
        conn.execute_batch(SCHEMA_V5)?;
        conn.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', '5')",
            [],
        )?;
    }
    if version < 6 {
        // v6: query feedback loop — seed/visited node pairs per question,
        // promoted to `learned` edges when they recur across questions.
        conn.execute_batch(SCHEMA_V6)?;
        conn.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', '6')",
            [],
        )?;
    }
    if version < 7 {
        // v7: hyperedges — N-ary group relationships (communities, shared
        // reference groups). nodes is a JSON array of member node ids.
        conn.execute_batch(SCHEMA_V7)?;
        conn.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', '7')",
            [],
        )?;
    }
    if version < 8 {
        // v8: cross-repo plumbing — nodes.metadata holds parked unresolved
        // call info for the global-graph resolver, nodes.repo tags the owning
        // repo in a merged global store, edges.context marks pass-generated
        // edges (e.g. cross_repo) so they can be re-run idempotently.
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN metadata TEXT;")?;
        conn.execute_batch("ALTER TABLE nodes ADD COLUMN repo TEXT;")?;
        conn.execute_batch("ALTER TABLE edges ADD COLUMN context TEXT;")?;
        conn.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', '8')",
            [],
        )?;
    }
    if version < 9 {
        // v9: LLM community enrichment + per-run LLM accounting.
        // communities.summary carries the one-line thematic description,
        // label_source says who named it ('hub' fallback vs 'llm'), and
        // member_hash caches the membership so labels are only recomputed
        // when the community actually changes. pipeline_runs gains the
        // measured token spend of the semantic passes.
        conn.execute_batch("ALTER TABLE communities ADD COLUMN summary TEXT;")?;
        conn.execute_batch(
            "ALTER TABLE communities ADD COLUMN label_source TEXT NOT NULL DEFAULT 'hub';",
        )?;
        conn.execute_batch("ALTER TABLE communities ADD COLUMN member_hash TEXT;")?;
        conn.execute_batch(
            "ALTER TABLE pipeline_runs ADD COLUMN llm_input_tokens INTEGER;
             ALTER TABLE pipeline_runs ADD COLUMN llm_output_tokens INTEGER;
             ALTER TABLE pipeline_runs ADD COLUMN llm_api_calls INTEGER;",
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', '9')",
            [],
        )?;
    }

    Ok(())
}

pub fn open_db(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
    )?;
    let is_new = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        == 0;
    if is_new {
        conn.execute_batch(SCHEMA_V1)?;
    }
    run_migrations(&conn)?;
    Ok(conn)
}

pub fn open_db_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
    conn.execute_batch(SCHEMA_V1)?;
    run_migrations(&conn)?;
    Ok(conn)
}

/// Among all nodes sharing a bare name (label without `()`/leading `.`),
/// pick a real definition over a speculative stub. Stubs come from
/// unresolved edge targets and markdown identifiers; when one shadows a
/// code symbol, `affected`/`explain` resolve to a node with no edges and
/// report a false empty blast radius.
pub fn prefer_non_stub_id(conn: &Connection, bare: &str) -> Option<String> {
    conn.query_row(
        "SELECT id FROM nodes WHERE lower(replace(ltrim(label, '.'), '()', '')) = ?1
         ORDER BY file_type = 'stub', id LIMIT 1",
        rusqlite::params![bare],
        |r| r.get(0),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_creates_tables() {
        let conn = open_db_in_memory().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(tables.contains(&"nodes".to_string()));
        assert!(tables.contains(&"edges".to_string()));
        assert!(tables.contains(&"extraction_cache".to_string()));
        assert!(tables.contains(&"file_manifest".to_string()));
        assert!(tables.contains(&"pipeline_runs".to_string()));
        assert!(tables.contains(&"query_history".to_string()));
    }

    #[test]
    fn indexes_exist() {
        let conn = open_db_in_memory().unwrap();
        let indexes: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(indexes.contains(&"idx_nodes_file".to_string()));
        assert!(indexes.contains(&"idx_nodes_community".to_string()));
        assert!(indexes.contains(&"idx_edges_source".to_string()));
        assert!(indexes.contains(&"idx_edges_target".to_string()));
    }

    #[test]
    fn insert_and_query_node() {
        let conn = open_db_in_memory().unwrap();
        conn.execute(
            "INSERT INTO nodes (id, label, file_type, source_file, source_line) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["main.py::Foo", "Foo", "code", "main.py", 10],
        ).unwrap();

        let label: String = conn
            .query_row(
                "SELECT label FROM nodes WHERE id = ?1",
                rusqlite::params!["main.py::Foo"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(label, "Foo");
    }

    #[test]
    fn schema_v3_has_signature_column() {
        let conn = open_db_in_memory().unwrap();
        let version: String = conn
            .query_row(
                "SELECT value FROM _meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, "9");
        conn.execute(
            "INSERT INTO nodes (id, label, file_type, source_file, signature) VALUES ('a', 'A', 'code', 'f.rs', 'fn a()')",
            [],
        )
        .unwrap();
        let sig: Option<String> = conn
            .query_row("SELECT signature FROM nodes WHERE id = 'a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(sig.as_deref(), Some("fn a()"));
    }

    #[test]
    fn schema_v5_has_node_embeddings() {
        let conn = open_db_in_memory().unwrap();
        conn.execute(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('a', 'A', 'code', 'f.rs')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO node_embeddings (node_id, dim, embedding, model, embedded_at)
             VALUES ('a', 2, X'0000803F' || X'00000000', 'test-model', '2026-01-01')",
            [],
        )
        .unwrap();
        let (dim, model): (i64, String) = conn
            .query_row(
                "SELECT dim, model FROM node_embeddings WHERE node_id = 'a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((dim, model.as_str()), (2, "test-model"));
        // ON DELETE CASCADE drops vectors with their nodes
        conn.execute("DELETE FROM nodes WHERE id = 'a'", [])
            .unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM node_embeddings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn schema_v4_has_edge_source_line() {
        let conn = open_db_in_memory().unwrap();
        conn.execute(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('a', 'A', 'code', 'f.rs'), ('b', 'B', 'code', 'f.rs')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO edges (source, target, relation, confidence, source_file, source_line) VALUES ('a', 'b', 'calls', 'EXTRACTED', 'f.rs', 7)",
            [],
        )
        .unwrap();
        let line: Option<i64> = conn
            .query_row(
                "SELECT source_line FROM edges WHERE source = 'a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(line, Some(7));
    }
}
