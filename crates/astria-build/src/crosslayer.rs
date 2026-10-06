// crosslayer: deterministic post-build linking passes over the whole DB.
// They bridge layers the per-file extractors cannot see, because each
// extractor only ever holds one file's symbols:
//
// - docs that name a package → `references` edges to the package node
//   (the crate table in architecture.md, README package lists, ...)
// - packages → `entry_point` edges to their entry source file, so the
//   package/dependency layer connects to actual code
// - TS/JS files importing the napi binding → `ffi_binding` edges to the
//   Rust functions behind them (napi-rs exports snake_case fns as
//   camelCase JS names — an FFI boundary no AST in either language sees)
//
// Every edge carries context='crosslayer' and is replaced wholesale on
// each pipeline run: the passes re-derive from the current DB, so
// incremental updates and build-deleted edges are restored, never
// accumulated.

use std::collections::{HashMap, HashSet};

use astria_core::ids::normalize_id;
use astria_core::Result;
use regex::Regex;
use rusqlite::Connection;

/// Max bytes of a doc/TS source file the passes will read.
const MAX_SOURCE_BYTES: usize = 1024 * 1024;

/// The TS/JS import shapes the FFI pass matches. A static literal compiled
/// once per process: the per-call `Regex::new(..).unwrap()` this replaces
/// was both a latent panic site and repeated compilation work.
static NATIVE_IMPORT_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(
        r#"(?s)(?:import|export)\s*(?:type\s+)?\{(?P<names>[^}]+)\}\s*from\s*['"][^'"]*native['"]"#,
    )
    .expect("valid static regex literal")
});

/// Entry-file conventions per ecosystem, tried in order. Workspace-root
/// manifests have none of these under them, so they link nothing.
const ENTRY_SUFFIXES: &[&str] = &[
    "src/lib.rs",
    "src/main.rs",
    "src/index.ts",
    "src/index.js",
    "index.ts",
    "index.js",
    "index.mjs",
    "src/__init__.py",
    "__init__.py",
    "main.py",
    "main.go",
];

#[derive(Debug, Default, PartialEq)]
pub struct CrossLayerStats {
    /// TS/JS symbols bound to their backing Rust functions.
    pub ffi_bindings: usize,
    /// Package → entry-file edges added.
    pub entry_points: usize,
    /// Doc → package reference edges added.
    pub doc_refs: usize,
    /// Speculative nodes whose stale file locus was cleared (see
    /// `normalize_stub_loci`).
    pub stub_loci_cleared: usize,
}

/// Relations this pass emits, in insert order. `scripts/check-docs-sync.mjs`
/// parses this list — every new relation must land here AND in
/// ARCHITECTURE.md's relationship section.
pub const EMITTED_RELATIONS: &[&str] = &["ffi_binding", "entry_point", "references"];

impl CrossLayerStats {
    pub fn total(&self) -> usize {
        self.ffi_bindings + self.entry_points + self.doc_refs
    }
}

/// Speculative nodes (`stub`, `reference`) exist for names the extractors
/// could not resolve to a definition. They have no owning file by
/// construction: a stub is created exactly when no file defined the symbol,
/// and a reference is a dependency NAME rather than code.
///
/// Borrowing the referencing file's path gave these nodes a plausible-looking
/// locus that consumers treat as a real source location, so one
/// `import rusqlite` produced a `health.rs → <whatever file first mentioned
/// rusqlite>` file dependency, a four-crate "import cycle", a false `File:`
/// line in `explain`, and thousands of cross-community "surprising
/// connections".
///
/// Enforced as a total invariant rather than a per-edge rule because a stub
/// can only ever have an EMPTY locus — the two cannot coexist — which makes
/// this idempotent and safe to re-run. Runs as a derived pass so an
/// incremental update heals stubs that older builds stamped, and returns the
/// number of nodes it cleared.
pub fn normalize_stub_loci(db: &Connection) -> Result<usize> {
    let cleared = db.execute(
        "UPDATE nodes SET source_file = '' WHERE file_type IN ('stub', 'reference') AND source_file != ''",
        [],
    )?;
    Ok(cleared)
}

pub fn link_cross_layer(db: &Connection) -> Result<CrossLayerStats> {
    let stub_loci_cleared = normalize_stub_loci(db)?;
    // The passes own this context wholesale: replace, never accumulate.
    db.execute("DELETE FROM edges WHERE context = 'crosslayer'", [])?;
    let stats = CrossLayerStats {
        ffi_bindings: link_napi_ffi(db)?,
        entry_points: link_entry_points(db)?,
        doc_refs: link_doc_package_refs(db)?,
        stub_loci_cleared,
    };
    Ok(stats)
}

/// Node id of the file-level node for a stored source path (single-segment
/// ids are file nodes; definition ids are `file::symbol`). Speculative nodes
/// are excluded: they carry no locus and no identity of their own.
fn file_node_id(db: &Connection, source_file: &str) -> Option<String> {
    db.query_row(
        "SELECT id FROM nodes WHERE source_file = ?1 AND id NOT LIKE '%::%' AND file_type NOT IN ('stub', 'reference') LIMIT 1",
        rusqlite::params![source_file],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

fn insert_edge(
    db: &Connection,
    source: &str,
    target: &str,
    relation: &str,
    score: f64,
    source_file: &str,
    source_line: Option<u32>,
) -> Result<()> {
    db.execute(
        "INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file, source_line, context)
         VALUES (?1, ?2, ?3, 'INFERRED', ?4, ?5, ?6, 'crosslayer')",
        rusqlite::params![source, target, relation, score, source_file, source_line],
    )?;
    Ok(())
}

/// napi FFI: files importing a module path ending in `native` are the JS
/// surface of a napi binding. Their imported identifiers mirror Rust
/// function names (camelCase ↔ snake_case), so an imported name that
/// matches exactly one Rust function links to it — from the in-file symbol
/// node that references it (typically the stub an unresolved call created),
/// or from the file node when no such symbol exists.
fn link_napi_ffi(db: &Connection) -> Result<usize> {
    // Files whose imports point at the binding. normalize_id strips dots and
    // slashes, so `../native` lands as target "native" and `x/native` as
    // "..._native".
    let mut importers: Vec<String> = Vec::new();
    {
        let mut stmt = db.prepare(
            "SELECT DISTINCT source_file FROM edges
             WHERE relation = 'imports' AND (target = 'native' OR target LIKE '%\\_native' ESCAPE '\\')",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for row in rows {
            importers.push(row?);
        }
    }
    let importers: Vec<String> = importers
        .into_iter()
        .filter(|f| {
            matches!(
                f.rsplit('.').next(),
                Some("ts") | Some("tsx") | Some("js") | Some("jsx") | Some("mjs") | Some("cjs")
            )
        })
        .collect();
    if importers.is_empty() {
        return Ok(0);
    }

    // Rust functions indexed by their snake_case name (the last `::`
    // segment of code nodes in .rs files). Ambiguous names link nothing —
    // same rule as cross-file call resolution.
    let mut rust_fns: HashMap<String, Vec<String>> = HashMap::new();
    {
        let mut stmt = db
            .prepare("SELECT id FROM nodes WHERE file_type = 'code' AND source_file LIKE '%.rs'")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for row in rows {
            let id: String = row?;
            if let Some(pos) = id.rfind("::") {
                rust_fns
                    .entry(id[pos + 2..].to_string())
                    .or_default()
                    .push(id);
            }
        }
    }

    let mut added = 0;
    let mut linked: HashSet<(String, String)> = HashSet::new();
    for ts_path in &importers {
        let Ok(text) = std::fs::read_to_string(ts_path) else {
            continue;
        };
        if text.len() > MAX_SOURCE_BYTES {
            continue;
        }
        let Some(file_id) = file_node_id(db, ts_path) else {
            continue;
        };
        for caps in NATIVE_IMPORT_RE.captures_iter(&text) {
            for ident in caps["names"].split(',') {
                let ident = ident
                    .trim()
                    .trim_start_matches("type ")
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                if ident.is_empty() {
                    continue;
                }
                let targets = match rust_fns.get(&camel_to_snake(ident)) {
                    Some(v) if v.len() == 1 => v,
                    _ => continue,
                };
                let target = targets[0].clone();
                // In-file symbol node referencing the import: a real
                // definition in this file, or the global stub an unresolved
                // call created. A stub for an imported binding has no file
                // locus of its own (it is a name, not a definition), so the
                // lookup cannot filter on source_file alone — an in-file
                // definition still wins when one exists.
                let ident_norm = normalize_id(ident);
                let mut bound = false;
                {
                    let mut stmt = db.prepare(
                        "SELECT id FROM nodes WHERE (id = ?2 OR id LIKE '%::' || ?2)\r\n                         AND (source_file = ?1 OR source_file = '')\r\n                         ORDER BY (source_file = ?1) DESC LIMIT 3",
                    )?;
                    let rows = stmt.query_map(rusqlite::params![ts_path, ident_norm], |r| {
                        r.get::<_, String>(0)
                    })?;
                    for row in rows {
                        let sym_id: String = row?;
                        if linked.insert((sym_id.clone(), target.clone())) {
                            insert_edge(db, &sym_id, &target, "ffi_binding", 0.9, ts_path, None)?;
                            added += 1;
                        }
                        bound = true;
                    }
                }
                if !bound && linked.insert((file_id.clone(), target.clone())) {
                    insert_edge(db, &file_id, &target, "ffi_binding", 0.8, ts_path, None)?;
                    added += 1;
                }
            }
        }
    }
    Ok(added)
}

/// camelCase JS binding name → snake_case Rust name ("runMcpServer" →
/// "run_mcp_server"). An underscore goes before an uppercase letter that
/// follows a lowercase/digit, or starts an acronym tail ("loadURL" →
/// "load_url").
fn camel_to_snake(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            let prev_soft = i > 0 && (chars[i - 1].is_lowercase() || chars[i - 1].is_ascii_digit());
            let next_soft = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if i > 0 && (prev_soft || next_soft) && !out.ends_with('_') {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Package → entry-point file: for each known package, the conventional
/// entry file named after it anywhere in the tree (crates named like their
/// directory, python packages normalized to snake_case, ...). Ambiguous
/// matches (two crates claiming the same entry path) link nothing.
fn link_entry_points(db: &Connection) -> Result<usize> {
    let mut packages: Vec<(String, String)> = {
        let mut stmt = db.prepare("SELECT id, label FROM nodes WHERE file_type = 'package'")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    packages.sort();
    packages.dedup();

    let mut added = 0;
    for (pkg_id, label) in packages {
        // npm scoped names and paths have no directory of their own.
        if label.contains('/') || label.starts_with('@') || label.trim().is_empty() {
            continue;
        }
        let dir = normalize_id(&label);
        for suffix in ENTRY_SUFFIXES {
            let pattern = format!("%/{dir}/{suffix}");
            let mut stmt = db.prepare(
                "SELECT id FROM nodes WHERE source_file LIKE ?1 AND id NOT LIKE '%::%' AND file_type != 'stub'
                 ORDER BY id LIMIT 2",
            )?;
            let rows: Vec<String> = {
                let mapped =
                    stmt.query_map(rusqlite::params![pattern], |r| r.get::<_, String>(0))?;
                mapped.filter_map(|r| r.ok()).collect()
            };
            if rows.len() == 1 {
                insert_edge(db, &pkg_id, &rows[0], "entry_point", 0.9, &pkg_id, Some(1))?;
                added += 1;
            }
            if !rows.is_empty() {
                break;
            }
        }
    }
    Ok(added)
}

/// Docs → packages: a document that names a package as a whole token
/// (crate tables, package lists, code fences) gets a `references` edge to
/// the package node. Names short or generic-looking (len < 5 and no `-`/`_`)
/// are skipped — they would drown prose in false matches.
fn link_doc_package_refs(db: &Connection) -> Result<usize> {
    let mut packages: Vec<(String, String)> = {
        let mut stmt = db.prepare("SELECT label, id FROM nodes WHERE file_type = 'package'")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    packages.sort();
    packages.dedup();
    let names: Vec<(&str, &str)> = packages
        .iter()
        .map(|(label, id)| (label.as_str(), id.as_str()))
        .filter(|(label, _)| {
            !label.trim().is_empty()
                && !label.contains(char::is_whitespace)
                && (label.len() >= 5 || label.contains('-') || label.contains('_'))
        })
        .collect();
    if names.is_empty() {
        return Ok(0);
    }

    // Longest-first alternation; \b handles word characters, the manual
    // byte check below rejects the hyphen continuation \b allows through
    // ("astria-mcp" must not match inside "astria-mcp-server").
    let mut escaped: Vec<String> = names.iter().map(|(l, _)| regex::escape(l)).collect();
    escaped.sort_by_key(|e| std::cmp::Reverse(e.len()));
    // Data-dependent pattern: the alternation spans every package label in
    // the DB, and very large corpora can exceed the regex engine's compiled
    // size limit. Report it, don't panic.
    let name_re = Regex::new(&format!(r"(?i)\b({})\b", escaped.join("|"))).map_err(|e| {
        astria_core::AstriaError::Graph(format!(
            "package-name regex failed to compile over {n} package labels: {e}",
            n = names.len()
        ))
    })?;

    let mut docs: Vec<String> = {
        let mut stmt = db.prepare(
            "SELECT DISTINCT source_file FROM nodes WHERE file_type = 'document'
             AND (source_file LIKE '%.md' OR source_file LIKE '%.mdx'
                  OR source_file LIKE '%.txt' OR source_file LIKE '%.rst')",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    docs.sort();

    let is_name_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
    let mut added = 0;
    for doc_path in &docs {
        let Ok(text) = std::fs::read_to_string(doc_path) else {
            continue;
        };
        if text.len() > MAX_SOURCE_BYTES {
            continue;
        }
        let Some(doc_id) = file_node_id(db, doc_path) else {
            continue;
        };
        let mut seen: HashSet<&str> = HashSet::new();
        'lines: for (line_no, line) in text.lines().enumerate() {
            for caps in name_re.captures_iter(line) {
                let m = caps.get(1).unwrap();
                let bytes = line.as_bytes();
                let before_ok = m.start() == 0 || !is_name_char(bytes[m.start() - 1]);
                let after_ok = m.end() == bytes.len() || !is_name_char(bytes[m.end()]);
                if !(before_ok && after_ok) {
                    continue;
                }
                let matched = line[m.start()..m.end()].to_lowercase();
                let Some((_, pkg_id)) = names.iter().find(|(l, _)| l.to_lowercase() == matched)
                else {
                    continue;
                };
                if seen.insert(pkg_id) {
                    match insert_edge(
                        db,
                        &doc_id,
                        pkg_id,
                        "references",
                        0.8,
                        doc_path,
                        Some(line_no as u32 + 1),
                    ) {
                        Ok(()) => added += 1,
                        Err(e) => {
                            eprintln!("warning: doc reference edge failed: {e}");
                            break 'lines;
                        }
                    }
                }
                if seen.len() >= 200 {
                    break 'lines;
                }
            }
        }
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize;
    use std::fs;
    use std::path::PathBuf;

    fn db() -> Connection {
        astria_core::db::open_db_in_memory().unwrap()
    }

    fn node(db: &Connection, id: &str, label: &str, file_type: &str, source_file: &str) {
        db.execute(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, label, file_type, source_file],
        )
        .unwrap();
    }

    fn import_edge(db: &Connection, source_id: &str, source_file: &str, target: &str) {
        // Edges reference existing nodes — mirror build's stub for the
        // unresolved import target.
        node(db, target, target, "stub", source_file);
        db.execute(
            "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES (?1, ?2, 'imports', 'EXTRACTED', ?3)",
            rusqlite::params![source_id, target, source_file],
        )
        .unwrap();
    }

    fn edge_count(db: &Connection, relation: &str) -> i64 {
        db.query_row(
            "SELECT COUNT(*) FROM edges WHERE relation = ?1 AND context = 'crosslayer'",
            rusqlite::params![relation],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn write(path: &PathBuf, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn native_import_binds_to_its_rust_function() {
        let dir = tempfile::tempdir().unwrap();
        let ts = dir.path().join("src/commands/mcp.ts");
        write(
            &ts,
            "import { runMcpServer } from '../native';\nexport async function mcpCommand() {\n  runMcpServer('g');\n}\n",
        );
        let ts_path = normalize(&ts);
        let db = db();
        node(&db, "src_commands_mcp", "mcp.ts", "code", &ts_path);
        // The unresolved binding is a global name; a stub carries no file
        // locus, which is what `ensure_node_exists` now produces.
        node(&db, "runmcpserver", "runMcpServer", "stub", "");
        node(
            &db,
            "crates_napi_src_lib::run_mcp_server",
            "run_mcp_server()",
            "code",
            &format!("{}/crates/napi/src/lib.rs", normalize(dir.path())),
        );
        import_edge(&db, "src_commands_mcp", &ts_path, "native");

        let stats = link_cross_layer(&db).unwrap();
        assert_eq!(stats.ffi_bindings, 1);
        let (src, tgt): (String, String) = db
            .query_row(
                "SELECT source, target FROM edges WHERE relation = 'ffi_binding'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        // The stub the unresolved call created is the bound symbol, not the
        // bare file node — traversal from the stub reaches the Rust side.
        assert_eq!(src, "runmcpserver");
        assert_eq!(tgt, "crates_napi_src_lib::run_mcp_server");
    }

    #[test]
    fn ambiguous_rust_names_stay_unbound() {
        let dir = tempfile::tempdir().unwrap();
        let ts = dir.path().join("a.ts");
        write(&ts, "import { run } from '../native';\nrun();\n");
        let ts_path = normalize(&ts);
        let db = db();
        node(&db, "a", "a.ts", "code", &ts_path);
        node(&db, "x::run", "run()", "code", "/p/x/src/lib.rs");
        node(&db, "y::run", "run()", "code", "/p/y/src/lib.rs");
        import_edge(&db, "a", &ts_path, "native");

        let stats = link_cross_layer(&db).unwrap();
        assert_eq!(stats.ffi_bindings, 0);
        assert_eq!(edge_count(&db, "ffi_binding"), 0);
    }

    #[test]
    fn files_without_native_imports_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let ts = dir.path().join("a.ts");
        write(
            &ts,
            "import { runMcpServer } from './utils';\nrunMcpServer();\n",
        );
        let ts_path = normalize(&ts);
        let db = db();
        node(&db, "a", "a.ts", "code", &ts_path);
        node(
            &db,
            "x::run_mcp_server",
            "run_mcp_server()",
            "code",
            "/p/lib.rs",
        );
        import_edge(&db, "a", &ts_path, "utils");

        let stats = link_cross_layer(&db).unwrap();
        assert_eq!(stats.ffi_bindings, 0);
    }

    #[test]
    fn package_links_to_its_entry_file() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("crates/demo/src/lib.rs");
        write(&entry, "pub fn serve() {}\n");
        let db = db();
        node(&db, "pkg_demo", "demo", "package", "/anywhere/Cargo.toml");
        node(
            &db,
            "crates_demo_src_lib",
            "lib.rs",
            "code",
            &normalize(&entry),
        );

        let stats = link_cross_layer(&db).unwrap();
        assert_eq!(stats.entry_points, 1);
        let (src, tgt): (String, String) = db
            .query_row(
                "SELECT source, target FROM edges WHERE relation = 'entry_point'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (src.as_str(), tgt.as_str()),
            ("pkg_demo", "crates_demo_src_lib")
        );
    }

    #[test]
    fn package_without_entry_file_links_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let db = db();
        node(&db, "pkg_demo", "demo", "package", &normalize(dir.path()));
        // Only a symbol lives under demo/ — no file-level node, no entry.
        node(
            &db,
            "demo_src_lib::serve",
            "serve()",
            "code",
            &format!("{}/demo/src/lib.rs", normalize(dir.path())),
        );

        let stats = link_cross_layer(&db).unwrap();
        assert_eq!(stats.entry_points, 0);
    }

    #[test]
    fn bare_stubs_do_not_block_the_entry_edge() {
        // Unresolved call stubs carry the referencing file's source_file and
        // bare ids — they must not read as competing file-level nodes.
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("crates/demo/src/lib.rs");
        write(&entry, "pub fn serve() {}\n");
        let db = db();
        node(&db, "pkg_demo", "demo", "package", "/anywhere/Cargo.toml");
        node(
            &db,
            "crates_demo_src_lib",
            "lib.rs",
            "code",
            &normalize(&entry),
        );
        node(&db, "as_u64", "as_u64", "stub", &normalize(&entry));
        node(&db, "lock", "lock", "stub", &normalize(&entry));

        let stats = link_cross_layer(&db).unwrap();
        assert_eq!(stats.entry_points, 1);
    }

    #[test]
    fn doc_naming_a_package_gets_a_reference_edge() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("docs/architecture.md");
        write(
            &md,
            "# Architecture\n\n| Crate | Role |\n| `astria-mcp` | MCP stdio server exposing the graph to AI agents. |\n| `astria-mcp-server-tools` | unrelated |\n",
        );
        let db = db();
        node(
            &db,
            "docs_architecture",
            "architecture.md",
            "document",
            &normalize(&md),
        );
        node(
            &db,
            "pkg_astria_mcp",
            "astria-mcp",
            "package",
            "/x/Cargo.toml",
        );

        let stats = link_cross_layer(&db).unwrap();
        assert_eq!(
            stats.doc_refs, 1,
            "longer name must not match through the hyphen"
        );
        let (src, tgt, line): (String, String, u32) = db
            .query_row(
                "SELECT source, target, source_line FROM edges WHERE relation = 'references' AND context = 'crosslayer'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<u32>>(2)?.unwrap())),
            )
            .unwrap();
        assert_eq!(
            (src.as_str(), tgt.as_str()),
            ("docs_architecture", "pkg_astria_mcp")
        );
        assert_eq!(line, 4, "edge points at the naming line");
    }

    #[test]
    fn rerun_replaces_instead_of_accumulating() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("docs/a.md");
        write(&md, "uses astria-mcp here.\n");
        let db = db();
        node(&db, "docs_a", "a.md", "document", &normalize(&md));
        node(
            &db,
            "pkg_astria_mcp",
            "astria-mcp",
            "package",
            "/x/Cargo.toml",
        );

        let first = link_cross_layer(&db).unwrap();
        let second = link_cross_layer(&db).unwrap();
        assert_eq!(first.doc_refs, 1);
        assert_eq!(second, first, "idempotent re-run");
        assert_eq!(edge_count(&db, "references"), 1);
    }

    #[test]
    fn inserted_relations_are_the_declared_ones() {
        // The docs-sync guard trusts EMITTED_RELATIONS; this keeps the list
        // honest against what the passes actually insert.
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("docs/a.md");
        write(&md, "mentions astria-mcp.\n");
        let ts = dir.path().join("crates/napi.ts");
        write(
            &ts,
            "import { runMcpServer } from '../native';\nrunMcpServer();\n",
        );
        let entry = dir.path().join("crates/astria-mcp/src/lib.rs");
        write(&entry, "pub fn serve() {}\n");
        let db = db();
        node(&db, "docs_a", "a.md", "document", &normalize(&md));
        node(
            &db,
            "pkg_astria_mcp",
            "astria-mcp",
            "package",
            "/x/Cargo.toml",
        );
        node(&db, "crates_napi", "napi.ts", "code", &normalize(&ts));
        node(
            &db,
            "crates_napi::run_mcp_server",
            "run_mcp_server()",
            "code",
            "/p/crates/napi/src/lib.rs",
        );
        node(
            &db,
            "crates_astria_mcp_src_lib",
            "lib.rs",
            "code",
            &normalize(&entry),
        );
        import_edge(&db, "crates_napi", &normalize(&ts), "native");

        link_cross_layer(&db).unwrap();
        let mut stmt = db
            .prepare("SELECT DISTINCT relation FROM edges WHERE context = 'crosslayer'")
            .unwrap();
        let used: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            !used.is_empty() && used.iter().all(|r| EMITTED_RELATIONS.contains(&r.as_str())),
            "every inserted relation must be declared: {used:?} vs {EMITTED_RELATIONS:?}"
        );
    }

    #[test]
    fn stub_loci_are_cleared_and_real_nodes_untouched() {
        // Legacy graphs stamped a stub with whichever file referenced it first
        // (the measured `console_error` claimed cli.test.ts). The invariant
        // pass clears those, leaves real nodes and already-empty stubs alone,
        // and is idempotent so an incremental update can re-run it safely.
        let db = db();
        node(&db, "stale", "console_error", "stub", "src/cli.test.ts");
        node(&db, "clean", "assert", "stub", "");
        node(&db, "ref", "child_process", "reference", "src/a.ts");
        node(&db, "real", "run()", "code", "src/lib.rs");

        assert_eq!(normalize_stub_loci(&db).unwrap(), 2);
        let locus = |id: &str| -> String {
            db.query_row("SELECT source_file FROM nodes WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_eq!(locus("stale"), "", "a stub must not claim a file");
        assert_eq!(locus("ref"), "", "a reference must not claim a file");
        assert_eq!(locus("clean"), "");
        assert_eq!(
            locus("real"),
            "src/lib.rs",
            "real nodes keep their own file"
        );
        assert_eq!(normalize_stub_loci(&db).unwrap(), 0, "idempotent");
    }

    #[test]
    fn camel_to_snake_conversions() {
        assert_eq!(camel_to_snake("runMcpServer"), "run_mcp_server");
        assert_eq!(camel_to_snake("loadURL"), "load_url");
        assert_eq!(camel_to_snake("parse"), "parse");
        assert_eq!(camel_to_snake("HTMLParser"), "html_parser");
    }
}
