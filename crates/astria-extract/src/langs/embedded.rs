// Embedded-script component extraction: Vue SFCs, Svelte components and Astro
// files carry their logic in `<script>` blocks (or `---` frontmatter for
// Astro). No tree-sitter grammar for the host format is wired in; instead the
// embedded TS/JS is extracted with the existing JavaScript/TypeScript
// extractors and re-attributed to the component file.

use std::fs;
use std::path::Path;

use astria_core::{AstriaError, Result};
use regex::Regex;

use crate::naming::{file_stem, make_node_id};
use crate::schema::{ExtractedNode, Extraction};

/// Extract a `.vue` / `.svelte` / `.astro` file: one `file` node for the
/// component plus every class/function/import the embedded TS/JS declares.
pub fn extract_component(path: &Path, naming: &Path) -> Result<Extraction> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let language = match ext.as_str() {
        "vue" => "Vue",
        "svelte" => "Svelte",
        "astro" => "Astro",
        other => {
            return Err(AstriaError::Parse {
                file: path.display().to_string(),
                message: format!("embedded extractor: unsupported extension .{other}"),
            })
        }
    };

    let source = fs::read_to_string(path).map_err(|e| {
        AstriaError::Io(std::io::Error::new(
            e.kind(),
            format!("Cannot read component {}: {e}", path.display()),
        ))
    })?;

    let fid = file_stem(naming);
    let file_id = make_node_id(&[&fid]);

    let mut nodes = vec![ExtractedNode {
        id: file_id.clone(),
        label: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string(),
        source_file: path.to_path_buf(),
        source_line: None,
        docstring: None,
        signature: None,
        node_type: "file".to_string(),
    }];
    let mut edges = Vec::new();

    for (index, segment) in segments(&source, &ext).into_iter().enumerate() {
        let is_ts = segment.is_typescript;
        let cfg = if is_ts {
            crate::langs::typescript::config()
        } else {
            crate::langs::javascript::config()
        };
        let seg_ext = if is_ts { "ts" } else { "js" };

        // Write the block to a scratch file so the standard per-file extractor
        // can parse it; the scratch file node is dropped and ids are
        // namespaced under the component so parallel segments cannot collide.
        let scratch_dir = std::env::temp_dir().join(format!(
            "astria-embedded-{}-{}-{index}",
            std::process::id(),
            fid
        ));
        fs::create_dir_all(&scratch_dir).map_err(|e| {
            AstriaError::Io(std::io::Error::new(
                e.kind(),
                format!("embedded scratch dir: {e}"),
            ))
        })?;
        let scratch = scratch_dir.join(format!("segment.{seg_ext}"));
        fs::write(&scratch, &segment.body).map_err(|e| {
            AstriaError::Io(std::io::Error::new(
                e.kind(),
                format!("embedded scratch file: {e}"),
            ))
        })?;
        let mut extraction = match crate::walkers::extract_single(&scratch, cfg, naming) {
            Ok(e) => e,
            Err(err) => {
                let _ = fs::remove_dir_all(&scratch_dir);
                return Err(err);
            }
        };
        let _ = fs::remove_dir_all(&scratch_dir);

        // Segment node ids are already namespaced under the component file
        // (the shared `naming` root), so identical symbols declared in two
        // segments collapse into one id and the dedup pass below merges them.
        for mut node in extraction.nodes.drain(..) {
            if node.node_type == "file" {
                continue;
            }
            node.source_file = path.to_path_buf();
            nodes.push(node);
        }
        for mut edge in extraction.edges {
            edge.source_file = path.to_path_buf();
            edges.push(edge);
        }
    }

    // Deduplicate by id (repeat declarations across segments).
    let mut seen = std::collections::HashSet::new();
    nodes.retain(|n| seen.insert(n.id.clone()));
    let mut seen_edges = std::collections::HashSet::new();
    edges.retain(|e| seen_edges.insert((e.source.clone(), e.target.clone(), e.relation.clone())));

    Ok(Extraction {
        file_path: path.to_path_buf(),
        language: language.to_string(),
        nodes,
        edges,
    })
}

struct Segment {
    body: String,
    is_typescript: bool,
}

static ASTRO_FRONTMATTER_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"(?s)\A---\r?\n(.*?)\r?\n---").expect("static regex"));
static SCRIPT_SEGMENT_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r#"(?is)<script\b([^>]*)>(.*?)</script>"#).expect("static regex")
});

fn segments(source: &str, ext: &str) -> Vec<Segment> {
    if ext == "astro" {
        // Astro frontmatter: a `---` fenced TS block at the top of the file.
        return match ASTRO_FRONTMATTER_RE.captures(source) {
            Some(caps) => vec![Segment {
                body: caps[1].to_string(),
                is_typescript: true,
            }],
            None => Vec::new(),
        };
    }

    static TS_LANG_ATTR: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r##"lang\s*=\s*["']?ts["']?"##).expect("static regex")
    });
    let mut out = Vec::new();
    for caps in SCRIPT_SEGMENT_RE.captures_iter(source) {
        let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let is_typescript = TS_LANG_ATTR.is_match(attrs);
        out.push(Segment {
            body: caps[2].to_string(),
            is_typescript,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    use astria_core::db::open_db_in_memory;

    fn extract_named(name: &str, source: &str) -> Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(name);
        fs::write(&file, source).unwrap();
        let db = open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn vue_script_setup_symbols_belong_to_the_component() {
        let ext = extract_named(
            "App.vue",
            "<template>\n  <div>{{ greeting }}</div>\n</template>\n<script setup lang=\"ts\">\ninterface Props {\n  name: string\n}\nfunction greeting(): string {\n  return 'hi'\n}\n</script>\n",
        );
        assert_eq!(ext.language, "Vue");
        assert!(ext
            .nodes
            .iter()
            .any(|n| n.node_type == "file" && n.label == "App.vue"));
        assert!(
            ext.nodes.iter().any(|n| n.label.contains("greeting")),
            "embedded function: {:?}",
            ext.nodes.iter().map(|n| &n.label).collect::<Vec<_>>()
        );
        assert!(
            ext.nodes
                .iter()
                .any(|n| n.source_file.extension().and_then(|e| e.to_str()) == Some("vue")),
            "nodes must be attributed to the .vue file"
        );
    }

    #[test]
    fn svelte_component_extracts_js_symbols() {
        let ext = extract_named(
            "Widget.svelte",
            "<script>\nexport function spin() {\n  return true\n}\n</script>\n<span>ok</span>\n",
        );
        assert_eq!(ext.language, "Svelte");
        assert!(ext.nodes.iter().any(|n| n.label.contains("spin")));
    }

    #[test]
    fn astro_frontmatter_extracts_ts_symbols() {
        let ext = extract_named(
            "index.astro",
            "---\nconst layout = 'base'\nexport function title(): string {\n  return 'Astria'\n}\n---\n<html><body>{title()}</body></html>\n",
        );
        assert_eq!(ext.language, "Astro");
        assert!(ext.nodes.iter().any(|n| n.label.contains("title")));
    }

    #[test]
    fn duplicate_symbols_across_segments_are_deduped() {
        let ext = extract_named(
            "Dup.vue",
            "<script>\nfunction once() { return 1 }\n</script>\n<script>\nfunction once() { return 2 }\n</script>\n",
        );
        let count = ext
            .nodes
            .iter()
            .filter(|n| n.label.contains("once"))
            .count();
        assert_eq!(count, 1, "deduped to one node: {:?}", ext.nodes);
    }

    #[test]
    fn unsupported_extension_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.html");
        fs::write(&file, "<p>hi</p>").unwrap();
        let db = open_db_in_memory().unwrap();
        let results = engine::extract(&[file.clone()], dir.path(), &db).unwrap();
        // .html is a document, not an embedded component - it must not reach
        // this extractor (routed by extension before the language fallthrough).
        assert!(!results.iter().any(|r| r.language == "Vue"));
        let _ = db;
    }
}
