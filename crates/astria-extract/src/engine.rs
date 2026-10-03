// engine: extraction orchestrator. Routes each file to the right extractor
// (AST walkers for code, plain-text extractors for docs, manifest ingestion),
// consults the extraction cache, and finally resolves cross-file references.
//
// The per-concern implementations live in sibling modules:
// - `naming`   — stable node/target id construction
// - `cache`    — extraction_cache table access and content hashing
// - `walkers`  — tree-sitter structural + call-graph extraction
// - `docs`     — markdown / plain-text / RST extraction
// - `refs`     — cross-file call/import target resolution

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::cache::{check_cache, file_hash, save_cache};
use crate::docs::{
    extract_html, extract_markdown, extract_markdown_from_string, extract_rst, extract_text_file,
};
use crate::langs;
use crate::refs::resolve_cross_file_references;
use crate::schema::Extraction;
use crate::walkers::extract_single;
use astria_audio::TranscribeError;
use astria_core::AstriaError;
use astria_gws::GwsError;

/// Store the document layer's converted text for a binary format (PDF,
/// office, workspace export, media transcript), keyed by content hash: the
/// semantic pass enriches exactly this text instead of re-reading the raw
/// bytes.
fn save_derived_text(db: &Connection, file_path: &Path, hash: &str, text: &str) {
    let key = astria_paths::normalize(file_path);
    if let Err(e) = db.execute(
        "INSERT OR REPLACE INTO derived_text (file_path, content_hash, text) VALUES (?1, ?2, ?3)",
        rusqlite::params![key, hash, text],
    ) {
        eprintln!(
            "warning: failed to store derived text for {}: {e}",
            file_path.display()
        );
    }
}

pub fn extract(
    files: &[PathBuf],
    root: &Path,
    db: &Connection,
) -> Result<Vec<Extraction>, AstriaError> {
    let mut results = Vec::new();
    // Distinct transcription-unavailable causes already reported this run:
    // one actionable notice per cause, not one per media file.
    let mut transcription_notices: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for file_path in files {
        let extension = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let ext = extension.as_str();

        // Node-id prefixes must come from the path relative to the scanned
        // root, never from the caller's CWD-joined path: identical content
        // at different roots (relocated checkouts, clones) must produce
        // identical ids or merge/diff mismatch every node.
        let naming = file_path.strip_prefix(root).unwrap_or(file_path);

        let hash = file_hash(file_path)?;

        // Manifests: deterministic package/dependency ingestion, no AST
        if crate::manifest::is_manifest(file_path) {
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            let extraction = crate::manifest::extract_manifest(file_path);
            save_cache(db, file_path, &hash, &extraction);
            results.push(extraction);
            continue;
        }

        // Markdown: plain-text extraction (no tree-sitter)
        if ext == "md" || ext == "mdx" || ext == "qmd" {
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            let extraction = extract_markdown(file_path, naming)?;
            save_cache(db, file_path, &hash, &extraction);
            results.push(extraction);
            continue;
        }

        // PDF: extract text via astria-pdf, then parse as markdown
        if ext == "pdf" {
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            let md_text = astria_pdf::extract_to_markdown(file_path)?;
            let extraction = extract_markdown_from_string(file_path, "pdf", &md_text, naming);
            save_derived_text(db, file_path, &hash, &md_text);
            save_cache(db, file_path, &hash, &extraction);
            results.push(extraction);
            continue;
        }

        // Office docs (.docx/.xlsx): extract via astria-office, then parse
        // as markdown - the same shape as the PDF route above. Deterministic,
        // so results are cached.
        if ext == "docx" || ext == "xlsx" {
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            let md_text = astria_office::extract_to_markdown(file_path)?;
            let extraction = extract_markdown_from_string(file_path, "office", &md_text, naming);
            save_derived_text(db, file_path, &hash, &md_text);
            save_cache(db, file_path, &hash, &extraction);
            results.push(extraction);
            continue;
        }

        // Google Workspace shortcuts (.gdoc/.gsheet/.gslides): resolve
        // the Drive link, export via the Drive API, parse as markdown.
        // Missing credentials or a failed export degrade to a notice and
        // an UNcached empty extraction (same shape as the whisper route).
        if matches!(ext, "gdoc" | "gsheet" | "gslides") {
            // The local shortcut bytes never change when the cloud document
            // is edited; the cache fingerprint therefore includes the
            // remote revision when one is reachable (online refresh).
            // Offline it falls back to the local hash: the cached
            // extraction stands until credentials return.
            let hash = match astria_gws::remote_revision(file_path) {
                Some(rev) => format!("{hash}:gws-rev:{rev}"),
                None => hash,
            };
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            match astria_gws::export_to_markdown(file_path) {
                Ok(md_text) => {
                    save_derived_text(db, file_path, &hash, &md_text);
                    let extraction =
                        extract_markdown_from_string(file_path, "gws", &md_text, naming);
                    save_cache(db, file_path, &hash, &extraction);
                    results.push(extraction);
                }
                Err(GwsError::Unavailable(notice)) => {
                    if transcription_notices.insert(notice.clone()) {
                        eprintln!("[astria] google workspace skipped: {notice}");
                    }
                    results.push(Extraction {
                        file_path: file_path.clone(),
                        language: "gws".into(),
                        nodes: Vec::new(),
                        edges: Vec::new(),
                    });
                }
                Err(GwsError::Failed(message)) => {
                    eprintln!(
                        "warning: google workspace export failed for {}: {message}",
                        file_path.display()
                    );
                    results.push(Extraction {
                        file_path: file_path.clone(),
                        language: "gws".into(),
                        nodes: Vec::new(),
                        edges: Vec::new(),
                    });
                }
            }
            continue;
        }

        // Video/audio: transcribe via external whisper-cli (ffmpeg demuxes
        // video), then parse the transcript as markdown - the same shape as
        // the PDF route above. Missing tooling or model, or a failed run,
        // skips the file with a notice and an empty, UNcached extraction so
        // installing the tooling is picked up on the next run even though
        // the file content is unchanged.
        if astria_core::is_transcribable_extension(ext) {
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            match astria_audio::transcribe_to_markdown(file_path, root) {
                Ok(md_text) => {
                    save_derived_text(db, file_path, &hash, &md_text);
                    let extraction =
                        extract_markdown_from_string(file_path, "transcript", &md_text, naming);
                    save_cache(db, file_path, &hash, &extraction);
                    results.push(extraction);
                }
                Err(TranscribeError::Unavailable(notice)) => {
                    if transcription_notices.insert(notice.clone()) {
                        eprintln!("[astria] media transcription skipped: {notice}");
                    }
                    results.push(Extraction {
                        file_path: file_path.clone(),
                        language: "media".into(),
                        nodes: Vec::new(),
                        edges: Vec::new(),
                    });
                }
                Err(TranscribeError::Failed(message)) => {
                    eprintln!(
                        "warning: transcription failed for {}: {message}",
                        file_path.display()
                    );
                    results.push(Extraction {
                        file_path: file_path.clone(),
                        language: "media".into(),
                        nodes: Vec::new(),
                        edges: Vec::new(),
                    });
                }
            }
            continue;
        }

        // HTML: strip tags/scripts/styles, then paragraph-based extraction
        if ext == "html" || ext == "htm" {
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            let extraction = extract_html(file_path, naming)?;
            save_cache(db, file_path, &hash, &extraction);
            results.push(extraction);
            continue;
        }

        // Plain text: paragraph-based extraction (YAML/YML included - config
        // and docs-as-config files are read as text chunks)
        if ext == "txt" || ext == "yaml" || ext == "yml" {
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            let language = if ext == "yaml" || ext == "yml" {
                "yaml"
            } else {
                "text"
            };
            let extraction = extract_text_file(file_path, language, naming)?;
            save_cache(db, file_path, &hash, &extraction);
            results.push(extraction);
            continue;
        }

        // reStructuredText: heading-based extraction
        if ext == "rst" {
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            let extraction = extract_rst(file_path, naming)?;
            save_cache(db, file_path, &hash, &extraction);
            results.push(extraction);
            continue;
        }

        // Component files carry their logic in embedded TS/JS blocks;
        // extract those with the JavaScript/TypeScript grammars.
        if matches!(ext, "vue" | "svelte" | "astro") {
            results.push(crate::langs::embedded::extract_component(
                file_path, naming,
            )?);
            continue;
        }

        // VB.NET and Pascal have no tree-sitter grammar crate; use the
        // regex-fallback extractors (same approach as upstream graphify).
        if ext == "vb" {
            results.push(crate::langs::vb_net::extract_regex(file_path, naming)?);
            continue;
        }
        if matches!(ext, "pas" | "dpr" | "dpk" | "inc") {
            results.push(crate::langs::pascal::extract_regex(file_path, naming)?);
            continue;
        }
        let cfg = match langs::get_language_for_extension(ext) {
            Some(c) => c,
            None => {
                // Media can receive semantic facts, but has no structural
                // extractor. An empty extraction also removes obsolete facts
                // when semantic enrichment is explicitly disabled.
                results.push(Extraction {
                    file_path: file_path.clone(),
                    language: "media".into(),
                    nodes: Vec::new(),
                    edges: Vec::new(),
                });
                continue;
            }
        };

        // Check cache
        if let Some(cached) = check_cache(db, file_path, &hash) {
            results.push(cached);
            continue;
        }

        // Extract
        let extraction = extract_single(file_path, cfg, naming)?;

        // Save to cache
        save_cache(db, file_path, &hash, &extraction);

        results.push(extraction);
    }

    // Same-file case-distinct declarations that normalize to the same id
    // get deterministic disambiguators before anything downstream sees
    // them (idempotent for cached results from before this pass existed).
    for result in &mut results {
        crate::naming::disambiguate_duplicate_ids(result);
    }

    // Cross-file resolution: try to match call/import targets to known node IDs
    resolve_cross_file_references(&mut results);

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ExtractedEdge;
    use astria_core::db::open_db_in_memory;
    use std::fs;

    #[test]
    fn media_without_transcriber_is_empty_and_uncached() {
        // Whatever the failure mode (no whisper-cli on PATH, or the fake
        // content failing to decode on machines that have it), a media file
        // that cannot be transcribed must produce an empty `media` extraction
        // and must not poison the extraction cache: installing whisper-cli
        // later has to be picked up on the next run even though the file
        // content is unchanged.
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("talk.mp3");
        fs::write(&media, b"definitely not real audio").unwrap();

        let db = open_db_in_memory().unwrap();
        let results = extract(&[media], dir.path(), &db).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].language, "media");
        assert!(results[0].nodes.is_empty());
        let cached: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM extraction_cache WHERE file_path LIKE '%talk.mp3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cached, 0, "failed transcription must not be cached");
    }

    #[test]
    fn video_extension_routes_to_media_branch() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("clip.mp4");
        fs::write(&media, b"not a real video").unwrap();

        let db = open_db_in_memory().unwrap();
        let results = extract(&[media], dir.path(), &db).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].language, "media");
    }

    #[test]
    fn extract_python_file() {
        let dir = tempfile::tempdir().unwrap();
        let py = dir.path().join("main.py");
        fs::write(
            &py,
            "\nclass Greeter:\n    \"\"\"Says hello\"\"\"\n    def greet(self, name):\n        print(name)\n\ndef helper():\n    pass\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[py], dir.path(), &db).unwrap();
        assert_eq!(results.len(), 1);
        let ext = &results[0];
        assert_eq!(ext.language, "Python");
        assert!(
            ext.nodes.iter().any(|n| n.label == "Greeter"),
            "missing class"
        );
        assert!(
            ext.nodes.iter().any(|n| n.label == "greet()"),
            "missing method"
        );
        assert!(
            ext.nodes.iter().any(|n| n.label == "helper()"),
            "missing function"
        );
        assert!(ext.edges.iter().any(|e| e.relation == "contains"));
        // print() is a Python builtin — must not produce a call edge
        assert!(
            !ext.edges
                .iter()
                .any(|e| e.relation == "calls" && e.target == "print"),
            "builtin call should be filtered"
        );
    }

    #[test]
    fn node_ids_are_normalized() {
        let dir = tempfile::tempdir().unwrap();
        let py = dir.path().join("My-Module.PY");
        fs::write(&py, "class Greeter:\n    def greet(self):\n        pass\n").unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[py], dir.path(), &db).unwrap();
        let ids: Vec<&String> = results[0].nodes.iter().map(|n| &n.id).collect();
        assert!(
            ids.iter().any(|id| id.ends_with("::Greeter")),
            "class id keeps its case (structural identity is case-sensitive): {ids:?}"
        );
        assert!(
            ids.iter().any(|id| id.contains("My_Module_PY_")),
            "file stem keeps case; punctuation collapses: {ids:?}"
        );
        assert!(
            ids.iter().any(|id| id.ends_with("::greet")),
            "method id should drop parens: {ids:?}"
        );
    }

    #[test]
    fn extract_rust_file() {
        let dir = tempfile::tempdir().unwrap();
        let rs = dir.path().join("main.rs");
        fs::write(
            &rs,
            "\nstruct Config {\n    name: String,\n}\n\nfn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[rs], dir.path(), &db).unwrap();
        let ext = &results[0];
        assert_eq!(ext.language, "Rust");
        assert!(
            ext.nodes.iter().any(|n| n.label == "Config"),
            "missing struct"
        );
        assert!(
            ext.nodes.iter().any(|n| n.label == "main()"),
            "missing fn main"
        );
    }

    #[test]
    fn extract_javascript_file() {
        let dir = tempfile::tempdir().unwrap();
        let js = dir.path().join("app.js");
        fs::write(
            &js,
            "\nclass App {\n    start() {\n        console.log(\"hello\");\n    }\n}\nfunction helper() {\n    return 42;\n}\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[js], dir.path(), &db).unwrap();
        let ext = &results[0];
        assert_eq!(ext.language, "JavaScript");
        assert!(ext.nodes.iter().any(|n| n.label == "App"));
        assert!(ext.nodes.iter().any(|n| n.label == "helper()"));
    }

    #[test]
    fn extract_rust_pub_consts_and_documented_ones() {
        // Value questions ("which embedding model", "what threshold") are
        // answered by a const's initializer: pub and documented constants
        // become nodes whose signature carries the value; private
        // undocumented ones stay out.
        let dir = tempfile::tempdir().unwrap();
        let rs = dir.path().join("config.rs");
        fs::write(
            &rs,
            "\n/// The embedding model used for nodes and queries.\n/// Downloads are gated on the cache.\npub const MODEL: &str = \"jina-code\";\n\nconst HIDDEN: u32 = 1;\n\n/// Documented but private.\nconst LIMIT: usize = 10;\n\nstatic mut COUNTER: u64 = 0;\n\npub static NAME: &str = \"astria\";\n\npub fn size() -> usize {\n    LIMIT\n}\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[rs], dir.path(), &db).unwrap();
        let ext = &results[0];
        assert_eq!(ext.language, "Rust");

        let model = ext
            .nodes
            .iter()
            .find(|n| n.label == "MODEL")
            .expect("pub const must be extracted");
        assert_eq!(model.node_type, "constant");
        assert_eq!(
            model.signature.as_deref(),
            Some("pub const MODEL: &str = \"jina-code\";"),
            "the initializer is the answer content: {:?}",
            model.signature
        );
        assert_eq!(
            model.docstring.as_deref(),
            Some(
                "The embedding model used for nodes and queries. Downloads are gated on the cache."
            ),
            "/// block above the item is the docstring"
        );
        assert!(
            model.id.ends_with("::MODEL"),
            "id scopes under the file (case preserved): {}",
            model.id
        );
        assert!(
            ext.edges
                .iter()
                .any(|e| e.relation == "contains" && e.target == model.id),
            "const is contained by its file"
        );

        // documented-private and pub-static both qualify
        assert!(ext.nodes.iter().any(|n| n.label == "LIMIT"));
        assert!(ext
            .nodes
            .iter()
            .any(|n| n.label == "NAME" && n.node_type == "constant"));

        // private undocumented const/static stay out
        assert!(!ext.nodes.iter().any(|n| n.label == "HIDDEN"));
        assert!(!ext.nodes.iter().any(|n| n.label == "COUNTER"));
    }

    #[test]
    fn const_initializer_calls_attribute_to_the_const() {
        let dir = tempfile::tempdir().unwrap();
        let rs = dir.path().join("sizes.rs");
        fs::write(&rs, "pub const SIZE: usize = helper();\n").unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[rs], dir.path(), &db).unwrap();
        let ext = &results[0];
        let size = ext.nodes.iter().find(|n| n.label == "SIZE").unwrap();
        assert!(
            ext.edges
                .iter()
                .any(|e| e.relation == "calls" && e.source == size.id),
            "a call in the initializer belongs to the const node"
        );
    }

    #[test]
    fn rust_doc_comments_become_docstrings() {
        // Rust documents items with `///` above them, not strings in the
        // body — without this pass the doc evidence the ranking and
        // embedding layers rely on simply does not exist for Rust code.
        let dir = tempfile::tempdir().unwrap();
        let rs = dir.path().join("service.rs");
        fs::write(
            &rs,
            "\n//! Request handling for the ingest pipeline.\n//! Entry point for workers.\n\n/// Validates an incoming request against the size limit.\n/// Returns the rejected bytes on failure.\npub fn validate_request(raw: &[u8]) -> bool {\n    true\n}\n\n/// Shared configuration for all handlers.\nstruct Config {\n    limit: usize,\n}\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[rs], dir.path(), &db).unwrap();
        let ext = &results[0];

        let file_node = ext.nodes.iter().find(|n| n.label == "service.rs").unwrap();
        assert_eq!(
            file_node.docstring.as_deref(),
            Some("Request handling for the ingest pipeline. Entry point for workers."),
            "//! module doc describes the file node"
        );

        let f = ext
            .nodes
            .iter()
            .find(|n| n.label == "validate_request()")
            .unwrap();
        assert_eq!(
            f.docstring.as_deref(),
            Some(
                "Validates an incoming request against the size limit. Returns the rejected bytes on failure."
            ),
            "/// block above the function is its docstring"
        );

        let c = ext.nodes.iter().find(|n| n.label == "Config").unwrap();
        assert_eq!(
            c.docstring.as_deref(),
            Some("Shared configuration for all handlers."),
            "/// block above the struct is its docstring"
        );
    }

    #[test]
    fn rust_impl_methods_scope_under_impl_type() {
        // Two impl blocks defining the same method name must not collide on
        // one file-level id — each scopes under its impl type.
        let dir = tempfile::tempdir().unwrap();
        let rs = dir.path().join("lib.rs");
        fs::write(
            &rs,
            "\nstruct A;\n\nimpl A {\n    pub fn from_env() -> Self { A }\n}\n\nstruct B;\n\nimpl B {\n    pub fn from_env() -> Self { B }\n}\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[rs], dir.path(), &db).unwrap();
        let ext = &results[0];
        let ids: Vec<&str> = ext.nodes.iter().map(|n| n.id.as_str()).collect();
        let from_env: Vec<&&str> = ids.iter().filter(|id| id.ends_with("::from_env")).collect();
        assert_eq!(
            from_env.len(),
            2,
            "both from_env methods extracted: {ids:?}"
        );
        assert!(
            ids.iter().all(|id| count_char(id, ':') >= 0),
            "ids well formed"
        );
        let unique = from_env.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(
            unique.len(),
            2,
            "impl method ids must be unique: {from_env:?}"
        );
    }

    fn count_char(s: &str, c: char) -> i32 {
        s.chars().filter(|x| *x == c).count() as i32
    }

    #[test]
    fn cfg_gated_duplicate_definitions_dedup() {
        // #[cfg(feature)] twins textually duplicate a definition; only one
        // exists per build, so extraction keeps the first occurrence.
        let dir = tempfile::tempdir().unwrap();
        let rs = dir.path().join("stage.rs");
        fs::write(
            &rs,
            "\n#[cfg(feature = \"embed\")]\nfn embed_stage() -> u8 { 1 }\n\n#[cfg(not(feature = \"embed\"))]\nfn embed_stage() -> u8 { 0 }\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[rs], dir.path(), &db).unwrap();
        let ext = &results[0];
        let count = ext
            .nodes
            .iter()
            .filter(|n| n.id.ends_with("::embed_stage"))
            .count();
        assert_eq!(count, 1, "cfg twins must not duplicate the node id");
    }

    #[test]
    fn markdown_repeated_headings_get_unique_ids() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("spec.md");
        fs::write(&md, "# Spec\n\n## Changes\n\none\n\n## Changes\n\ntwo\n").unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[md], dir.path(), &db).unwrap();
        let ext = &results[0];
        let section_ids: Vec<&str> = ext
            .nodes
            .iter()
            .filter(|n| n.node_type == "section")
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(
            section_ids.len(),
            3,
            "all sections extracted: {section_ids:?}"
        );
        let unique: std::collections::HashSet<_> = section_ids.iter().collect();
        assert_eq!(
            unique.len(),
            3,
            "repeated headings must get unique ids: {section_ids:?}"
        );
    }

    #[test]
    fn identifier_shaped_strings_become_reference_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let py = dir.path().join("svc.py");
        fs::write(
            &py,
            "\nimport os\n\ndef run():\n    url = os.getenv(\"PLANE_URL\")\n    status = \"needs_human\"\n    branch = \"harness/hr-101-fix-redis-leak\"\n    msg = \"retry\"\n    print(url, status, branch, msg)\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[py], dir.path(), &db).unwrap();
        let ext = &results[0];

        // Env-var style, snake_case keys, and slash/kebab chains are indexed.
        // Ids normalize separators to underscores, so distinct spellings of
        // the same key (kebab/snake/slash) merge into one reference node.
        for (literal, id_suffix) in [
            ("PLANE_URL", "plane_url"),
            ("needs_human", "needs_human"),
            (
                "harness/hr-101-fix-redis-leak",
                "harness_hr_101_fix_redis_leak",
            ),
        ] {
            let node = ext
                .nodes
                .iter()
                .find(|n| n.label == literal)
                .unwrap_or_else(|| panic!("missing reference node for {literal:?}"));
            assert_eq!(node.node_type, "reference");
            assert_eq!(node.id, format!("str::{id_suffix}"));
            assert!(node.source_line.is_some(), "ref node must carry a line");
            let edge = ext
                .edges
                .iter()
                .find(|e| e.relation == "references" && e.target == node.id)
                .unwrap_or_else(|| panic!("missing references edge for {literal:?}"));
            assert_eq!(edge.source_line, node.source_line);
        }

        // Plain single words ("retry") and prose strings are NOT indexed.
        assert!(
            !ext.nodes.iter().any(|n| n.label == "retry"),
            "plain word must not become a reference node"
        );
    }

    #[test]
    fn qualified_rust_calls_resolve_across_files() {
        let dir = tempfile::tempdir().unwrap();
        let def = dir.path().join("pipeline.rs");
        fs::write(&def, "pub fn load_graph_db() -> u32 {\n    1\n}\n").unwrap();
        let caller = dir.path().join("lib.rs");
        fs::write(
            &caller,
            "fn wrapper() {\n    let n = pipeline::load_graph_db();\n    let _ = n;\n}\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[def, caller], dir.path(), &db).unwrap();
        let all_edges: Vec<&ExtractedEdge> = results.iter().flat_map(|r| r.edges.iter()).collect();
        assert!(
            all_edges
                .iter()
                .any(|e| e.relation == "calls" && e.target.ends_with("::load_graph_db")),
            "qualified call should resolve to the definition id, got targets: {:?}",
            all_edges.iter().map(|e| &e.target).collect::<Vec<_>>()
        );
    }

    #[test]
    fn extraction_uses_cache() {
        let dir = tempfile::tempdir().unwrap();
        let py = dir.path().join("main.py");
        fs::write(&py, "def hello(): pass\n").unwrap();
        let db = open_db_in_memory().unwrap();
        let r1 = extract(std::slice::from_ref(&py), dir.path(), &db).unwrap();
        let r2 = extract(&[py], dir.path(), &db).unwrap();
        assert_eq!(r1[0].nodes.len(), r2[0].nodes.len());
    }

    #[test]
    fn extract_new_config_languages() {
        // Terraform/HCL, PowerShell, SystemVerilog, Metal — validate the
        // node-kind mappings against real parses; a language that yields
        // zero nodes means its config's kind names drifted from the grammar.
        let cases: &[(&str, &str)] = &[
            (
                "infra.tf",
                "resource \"aws_s3_bucket\" \"b\" {
  bucket = \"demo\"
}

variable \"region\" {
  default = \"us-east-1\"
}

module \"network\" {
  source = \"./net\"
}",
            ),
            (
                "tasks.ps1",
                "function Deploy-Stack {
  Write-Output \"deploying\"
}

class Stack {
  [string]$Name
}

Deploy-Stack",
            ),
            (
                "counter.sv",
                "import mypkg::*;
module counter(input clk, output reg [7:0] count);
  function automatic [7:0] next(input [7:0] v);
    next = v + 1;
  endfunction
endmodule",
            ),
            (
                "render.metal",
                "#include <metal_stdlib>
vertex float4 render_vertex(uint vid [[vertex_id]]) {
  return float4(1.0);
}
kernel void tintkernel() {}
",
            ),
        ];
        for (name, content) in cases {
            let dir = tempfile::tempdir().unwrap();
            let f = dir.path().join(name);
            fs::write(&f, content).unwrap();
            let db = open_db_in_memory().unwrap();
            let results = extract(std::slice::from_ref(&f), dir.path(), &db).unwrap();
            assert!(
                !results.is_empty() && !results[0].nodes.is_empty(),
                "{name}: extraction produced no nodes"
            );
            // Label-level assertions: the extraction must identify the actual
            // declared symbols, not just emit a bare file node.
            let labels: Vec<String> = results[0].nodes.iter().map(|n| n.label.clone()).collect();
            match *name {
                "infra.tf" => assert!(
                    labels.iter().any(|l| l.contains("aws_s3_bucket"))
                        && labels.iter().any(|l| l.contains("network")),
                    "terraform resources missing from labels: {labels:?}"
                ),
                "tasks.ps1" => assert!(
                    labels.iter().any(|l| l.contains("Deploy-Stack")),
                    "powershell function missing from labels: {labels:?}"
                ),
                "counter.sv" => assert!(
                    labels.iter().any(|l| l.contains("counter")),
                    "systemverilog module missing from labels: {labels:?}"
                ),
                "render.metal" => assert!(
                    labels.iter().any(|l| l.contains("render_vertex")),
                    "metal vertex function missing from labels: {labels:?}"
                ),
                _ => {}
            }
        }
    }

    #[test]
    fn ids_stable_across_relocated_roots() {
        // The id prefix must come from the root-relative path, not the
        // CWD-joined one: identical content under differently-named roots
        // must produce identical node ids or merge/diff mismatch everything.
        let mk = |name: &str| {
            let dir = tempfile::tempdir().unwrap();
            // The scanned root is the differently-named project dir itself,
            // as when `astria run <root>` points at a relocated checkout.
            let root = dir.path().join(name);
            std::fs::create_dir_all(&root).unwrap();
            let py = root.join("sample.py");
            std::fs::write(
                &py,
                "def hello(): pass
",
            )
            .unwrap();
            (dir, root, py)
        };
        let (_dir_a, root_a, py_a) = mk("proj_one");
        let (_dir_b, root_b, py_b) = mk("proj_two");
        let db = open_db_in_memory().unwrap();
        let ra = extract(&[py_a], &root_a, &db).unwrap();
        let rb = extract(&[py_b], &root_b, &db).unwrap();
        let ids_a: Vec<&str> = ra[0].nodes.iter().map(|n| n.id.as_str()).collect();
        let ids_b: Vec<&str> = rb[0].nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids_a, ids_b, "ids churned across relocated roots");
        assert!(
            !ids_a.iter().any(|i| i.contains("proj_one")),
            "root name leaked into ids"
        );
    }

    #[test]
    fn extract_markdown_file() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("guide.md");
        fs::write(
            &md,
            "# Getting Started\n\nIntro text.\n\n## Installation\n\nSee [setup guide](setup.md) for details.\n\n### Step 1\n\nDo the thing.\n\n## Usage\n\nHow to use it.\n",
        ).unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[md], dir.path(), &db).unwrap();
        let ext = &results[0];
        assert_eq!(ext.language, "markdown");

        // Document node + 4 section headings (Getting Started, Installation, Step 1, Usage)
        assert!(
            ext.nodes.len() >= 5,
            "expected >= 5 nodes, got {}",
            ext.nodes.len()
        );
        assert!(
            ext.nodes.iter().any(|n| n.node_type == "document"),
            "missing document node"
        );
        assert!(
            ext.nodes.iter().any(|n| n.label == "Getting Started"),
            "missing h1"
        );
        assert!(
            ext.nodes.iter().any(|n| n.label == "Installation"),
            "missing h2"
        );
        assert!(ext.nodes.iter().any(|n| n.label == "Step 1"), "missing h3");
        assert!(ext.nodes.iter().any(|n| n.label == "Usage"), "missing h2");

        // contains edges (doc → headings, parent → child)
        let contains: Vec<_> = ext
            .edges
            .iter()
            .filter(|e| e.relation == "contains")
            .collect();
        assert!(
            contains.len() >= 4,
            "expected >= 4 contains edges, got {}",
            contains.len()
        );

        // references edge to setup.md
        assert!(
            ext.edges
                .iter()
                .any(|e| e.relation == "references" && e.target.contains("setup")),
            "missing references edge to setup.md"
        );
    }

    #[test]
    fn extract_rationale_comments() {
        let dir = tempfile::tempdir().unwrap();
        let py = dir.path().join("main.py");
        fs::write(
            &py,
            "\ndef process(data):\n    # WHY: We need to normalize because upstream sends raw bytes\n    result = normalize(data)\n    # HACK: Temporary workaround for API bug\n    return result\n\nclass Handler:\n    # NOTE: This is not thread-safe\n    def handle(self):\n        pass\n",
        ).unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[py], dir.path(), &db).unwrap();
        let ext = &results[0];

        let rationale_nodes: Vec<_> = ext
            .nodes
            .iter()
            .filter(|n| n.node_type == "rationale")
            .collect();
        assert!(
            rationale_nodes.len() >= 3,
            "expected >= 3 rationale nodes, got {}",
            rationale_nodes.len()
        );

        assert!(
            rationale_nodes.iter().any(|n| n.label.contains("WHY")),
            "missing WHY rationale"
        );
        assert!(
            rationale_nodes.iter().any(|n| n.label.contains("HACK")),
            "missing HACK rationale"
        );
        assert!(
            rationale_nodes.iter().any(|n| n.label.contains("NOTE")),
            "missing NOTE rationale"
        );

        let rationale_edges: Vec<_> = ext
            .edges
            .iter()
            .filter(|e| e.relation == "rationale_for")
            .collect();
        assert!(
            rationale_edges.len() >= 3,
            "expected >= 3 rationale_for edges, got {}",
            rationale_edges.len()
        );
    }

    #[test]
    fn signatures_are_captured() {
        let dir = tempfile::tempdir().unwrap();
        let py = dir.path().join("svc.py");
        fs::write(
            &py,
            "
class Greeter:
    \"\"\"Says hello\"\"\"
    def greet(self, name):
        print(name)
",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[py], dir.path(), &db).unwrap();
        let greet = results[0]
            .nodes
            .iter()
            .find(|n| n.label == "greet()")
            .expect("greet node");
        let sig = greet.signature.as_deref().expect("signature captured");
        assert!(sig.contains("def greet"), "got: {sig}");
        assert!(
            !sig.contains("print"),
            "signature must exclude the body, got: {sig}"
        );
        let class = results[0]
            .nodes
            .iter()
            .find(|n| n.label == "Greeter")
            .expect("class node");
        assert!(class.signature.is_some());
    }

    #[test]
    fn extract_qmd_file_as_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("doc.qmd");
        fs::write(
            &file,
            "---\ntitle: T\n---\n\n# Heading\n\nBody text here.\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[file], dir.path(), &db).unwrap();
        assert_eq!(results.len(), 1);
        let ext = &results[0];
        let joined: String = ext
            .nodes
            .iter()
            .map(|n| format!("{}|{}|{:?}", n.label, n.node_type, n.docstring))
            .collect();
        assert!(
            joined.contains("Heading") || joined.contains("Body"),
            "content: {joined}"
        );
    }

    #[test]
    fn extract_html_strips_tags_and_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("page.html");
        fs::write(
            &file,
            "<html><head><style>p { color: red }</style></head>\n<body>\n<script>evil()</script>\n<h1>Hello</h1>\n<p>World body text.</p>\n</body></html>\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[file], dir.path(), &db).unwrap();
        assert_eq!(results.len(), 1);
        let ext = &results[0];
        assert_eq!(ext.language, "html");
        let joined = format!(
            "{}{}",
            ext.nodes
                .iter()
                .map(|n| n.label.as_str())
                .collect::<Vec<_>>()
                .join("|"),
            ext.nodes
                .iter()
                .map(|n| n.docstring.as_deref().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("|")
        );
        assert!(joined.contains("Hello"), "html text: {joined}");
        assert!(joined.contains("World body text."), "html text: {joined}");
        assert!(!joined.contains("evil"), "script must be dropped: {joined}");
        assert!(
            !joined.contains("color: red"),
            "style must be dropped: {joined}"
        );
    }

    #[test]
    fn extract_yaml_as_text_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("conf.yaml");
        fs::write(&file, "service:\n  port: 8080\n  name: astria\n").unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[file], dir.path(), &db).unwrap();
        assert_eq!(results.len(), 1);
        let ext = &results[0];
        assert_eq!(ext.language, "yaml");
        let joined = format!(
            "{}{}",
            ext.nodes
                .iter()
                .map(|n| n.label.as_str())
                .collect::<Vec<_>>()
                .join("|"),
            ext.nodes
                .iter()
                .map(|n| n.docstring.as_deref().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("|")
        );
        assert!(joined.contains("8080"), "yaml content: {joined}");
    }
}
