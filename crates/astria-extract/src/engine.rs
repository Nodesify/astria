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

use std::path::PathBuf;

use rusqlite::Connection;

use crate::cache::{check_cache, file_hash, save_cache};
use crate::docs::{extract_markdown, extract_markdown_from_string, extract_rst, extract_text_file};
use crate::langs;
use crate::refs::resolve_cross_file_references;
use crate::schema::Extraction;
use crate::walkers::extract_single;
use astria_core::AstriaError;

pub fn extract(files: &[PathBuf], db: &Connection) -> Result<Vec<Extraction>, AstriaError> {
    let mut results = Vec::new();

    for file_path in files {
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

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
        if ext == "md" || ext == "mdx" {
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            let extraction = extract_markdown(file_path)?;
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
            match astria_pdf::extract_to_markdown(file_path) {
                Ok(md_text) if !md_text.trim().is_empty() => {
                    let extraction = extract_markdown_from_string(file_path, "pdf", &md_text);
                    save_cache(db, file_path, &hash, &extraction);
                    results.push(extraction);
                }
                _ => {}
            }
            continue;
        }

        // Plain text: paragraph-based extraction
        if ext == "txt" {
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            if let Ok(extraction) = extract_text_file(file_path, "text") {
                save_cache(db, file_path, &hash, &extraction);
                results.push(extraction);
            }
            continue;
        }

        // reStructuredText: heading-based extraction
        if ext == "rst" {
            if let Some(cached) = check_cache(db, file_path, &hash) {
                results.push(cached);
                continue;
            }
            if let Ok(extraction) = extract_rst(file_path) {
                save_cache(db, file_path, &hash, &extraction);
                results.push(extraction);
            }
            continue;
        }

        let cfg = match langs::get_language_for_extension(ext) {
            Some(c) => c,
            None => continue,
        };

        // Check cache
        if let Some(cached) = check_cache(db, file_path, &hash) {
            results.push(cached);
            continue;
        }

        // Extract
        let extraction = extract_single(file_path, cfg)?;

        // Save to cache
        save_cache(db, file_path, &hash, &extraction);

        results.push(extraction);
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
    fn extract_python_file() {
        let dir = tempfile::tempdir().unwrap();
        let py = dir.path().join("main.py");
        fs::write(
            &py,
            "\nclass Greeter:\n    \"\"\"Says hello\"\"\"\n    def greet(self, name):\n        print(name)\n\ndef helper():\n    pass\n",
        )
        .unwrap();
        let db = open_db_in_memory().unwrap();
        let results = extract(&[py], &db).unwrap();
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
        let results = extract(&[py], &db).unwrap();
        let ids: Vec<&String> = results[0].nodes.iter().map(|n| &n.id).collect();
        assert!(
            ids.iter().any(|id| id.ends_with("::greeter")),
            "class id should be normalized lowercase: {ids:?}"
        );
        assert!(
            ids.iter().any(|id| id.contains("my_module")),
            "file stem 'My-Module' should normalize to my_module: {ids:?}"
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
        let results = extract(&[rs], &db).unwrap();
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
        let results = extract(&[js], &db).unwrap();
        let ext = &results[0];
        assert_eq!(ext.language, "JavaScript");
        assert!(ext.nodes.iter().any(|n| n.label == "App"));
        assert!(ext.nodes.iter().any(|n| n.label == "helper()"));
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
        let results = extract(&[rs], &db).unwrap();
        let ext = &results[0];
        let ids: Vec<&str> = ext.nodes.iter().map(|n| n.id.as_str()).collect();
        let from_env: Vec<&&str> = ids.iter().filter(|id| id.ends_with("::from_env")).collect();
        assert_eq!(from_env.len(), 2, "both from_env methods extracted: {ids:?}");
        assert!(
            ids.iter().all(|id| count_char(id, ':') >= 0),
            "ids well formed"
        );
        let unique = from_env.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), 2, "impl method ids must be unique: {from_env:?}");
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
        let results = extract(&[rs], &db).unwrap();
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
        let results = extract(&[md], &db).unwrap();
        let ext = &results[0];
        let section_ids: Vec<&str> = ext
            .nodes
            .iter()
            .filter(|n| n.node_type == "section")
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(section_ids.len(), 3, "all sections extracted: {section_ids:?}");
        let unique: std::collections::HashSet<_> = section_ids.iter().collect();
        assert_eq!(unique.len(), 3, "repeated headings must get unique ids: {section_ids:?}");
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
        let results = extract(&[py], &db).unwrap();
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
        let results = extract(&[def, caller], &db).unwrap();
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
        let r1 = extract(std::slice::from_ref(&py), &db).unwrap();
        let r2 = extract(&[py], &db).unwrap();
        assert_eq!(r1[0].nodes.len(), r2[0].nodes.len());
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
        let results = extract(&[md], &db).unwrap();
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
        let results = extract(&[py], &db).unwrap();
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
        let results = extract(&[py], &db).unwrap();
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
}
