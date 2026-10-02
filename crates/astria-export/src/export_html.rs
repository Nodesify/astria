use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

/// Single-file interactive graph viewer (canvas, community-bubble drill-down).
/// Source lives in packages/viewer; run `npm run build` there and commit the
/// rebuilt asset. Kept dependency-free and fully inlined so the exported page
/// also works in sandboxed HTML previewers with no network access.
const VIEWER_JS: &str = include_str!("../assets/viewer.js");
/// Maximum graph size accepted by the reference/standard HTML exporter.
pub const MAX_NODES_FOR_VIZ: usize = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlExportMode {
    Standard,
    Large,
}

impl HtmlExportMode {
    pub fn parse(value: &str) -> astria_core::Result<Self> {
        match value {
            "standard" => Ok(Self::Standard),
            "large" => Ok(Self::Large),
            other => Err(astria_core::AstriaError::Graph(format!(
                "invalid HTML export mode '{other}'; expected 'standard' or 'large'"
            ))),
        }
    }
}

pub fn export_html(db: &Connection, out_path: &Path) -> astria_core::Result<()> {
    export_html_with_mode(db, out_path, HtmlExportMode::Standard)
}

pub fn export_html_with_mode(
    db: &Connection,
    out_path: &Path,
    mode: HtmlExportMode,
) -> astria_core::Result<()> {
    let node_count: usize = db.query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))?;
    if mode == HtmlExportMode::Standard && node_count > MAX_NODES_FOR_VIZ {
        return Err(astria_core::AstriaError::Graph(format!(
            "graph has {node_count} nodes, exceeding the standard HTML visualization limit of {MAX_NODES_FOR_VIZ}; rerun with --mode large"
        )));
    }
    export_html_impl(db, out_path)
}

/// Distance between neighboring nodes inside a community, in px.
const NODE_SPACING: f64 = 38.0;
/// Extra gap between neighboring community centers, in px.
const COMMUNITY_GAP: f64 = 220.0;
/// Distinct hues in the palette; groups beyond this cycle through it.
const MAX_PALETTE: usize = 60;

const GOLDEN_ANGLE: f64 = 2.399_963_229_728_653;

#[derive(Serialize)]
struct NodeOut {
    id: String,
    label: String,
    #[serde(rename = "fileType")]
    file_type: String,
    #[serde(rename = "sourceFile")]
    source_file: String,
    #[serde(rename = "sourceLine")]
    source_line: Option<i64>,
    community: Option<i64>,
    color: String,
    x: f64,
    y: f64,
    degree: i64,
}

#[derive(Serialize)]
struct EdgeOut {
    from: String,
    to: String,
    /// Edge kind (calls / imports / references / ...) so the viewer can show
    /// what connects a focused node's neighbors.
    relation: String,
}

/// One community bubble: centroid from the precomputed layout, label from the
/// communities table (thematic when --label-communities ran) or a fallback.
#[derive(Serialize)]
struct CommunityOut {
    id: Option<i64>,
    label: String,
    size: usize,
    color: String,
    x: f64,
    y: f64,
}

#[derive(Serialize)]
struct HyperEdgeOut {
    id: String,
    label: String,
    nodes: Vec<String>,
}

#[derive(Serialize)]
struct Meta {
    #[serde(rename = "nodeCount")]
    node_count: usize,
    #[serde(rename = "edgeCount")]
    edge_count: usize,
    #[serde(rename = "communityCount")]
    community_count: usize,
}

#[derive(Serialize)]
struct Payload {
    nodes: Vec<NodeOut>,
    edges: Vec<EdgeOut>,
    communities: Vec<CommunityOut>,
    #[serde(rename = "hyperedges")]
    hyper_edges: Vec<HyperEdgeOut>,
    meta: Meta,
}

fn export_html_impl(db: &Connection, out_path: &Path) -> astria_core::Result<()> {
    let payload = build_payload(db)?;

    let data_json = serde_json::to_string(&payload)?;
    // Keep the embedded JSON from terminating the surrounding <script> block
    // (\u003c is valid in both JSON and JS string literals).
    let data_json = data_json
        .replace("</script", "\\u003c/script")
        .replace("<!--", "\\u003c!--");

    let viewer_js = VIEWER_JS.replace("</script", "\\u003c/script");

    let html = format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>astria &mdash; Knowledge Graph</title>
</head>
<body>
  <script>
    var DATA = {DATA_JSON};
  </script>
  <script>
    {VIEWER_JS}
  </script>
</body>
</html>"##,
        DATA_JSON = data_json,
        VIEWER_JS = viewer_js,
    );

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, html)?;
    Ok(())
}

fn build_payload(db: &Connection) -> astria_core::Result<Payload> {
    let mut degrees: HashMap<String, i64> = HashMap::new();
    let mut edges: Vec<EdgeOut> = Vec::new();
    {
        let mut stmt = db.prepare("SELECT source, target, COALESCE(relation, '') FROM edges")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let src: String = row.get(0)?;
            let tgt: String = row.get(1)?;
            let relation: String = row.get(2)?;
            *degrees.entry(src.clone()).or_insert(0) += 1;
            *degrees.entry(tgt.clone()).or_insert(0) += 1;
            edges.push(EdgeOut {
                from: src,
                to: tgt,
                relation,
            });
        }
    }
    let edge_count = edges.len();

    let mut stmt = db.prepare(
        "SELECT id, label, file_type, source_file, source_line, community FROM nodes ORDER BY id",
    )?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(String, String, String, String, Option<i64>, Option<i64>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    let node_count = rows.len();

    // Thematic community names live in the communities table when an
    // LLM-labeled run produced them; anything is fine here, so a missing
    // table (or a graph built before community labeling) falls back to ids.
    let mut community_labels: HashMap<i64, String> = HashMap::new();
    if let Ok(mut stmt) = db.prepare("SELECT id, label FROM communities") {
        if let Ok(labeled) = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        }) {
            for (id, label) in labeled.flatten() {
                community_labels.insert(id, label);
            }
        }
    }
    let label_of = |community: &i64| -> String {
        community_labels
            .get(community)
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Community {community}"))
    };

    // Communities ranked by size (ties broken by id for determinism); nodes
    // without a community form a trailing group of their own.
    let mut comm_counts: HashMap<i64, usize> = HashMap::new();
    for (_, _, _, _, _, comm) in &rows {
        if let Some(c) = comm {
            *comm_counts.entry(*c).or_insert(0) += 1;
        }
    }
    let mut comm_list: Vec<(i64, usize)> = comm_counts.into_iter().collect();
    comm_list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let group_index: HashMap<i64, usize> = comm_list
        .iter()
        .enumerate()
        .map(|(i, (c, _))| (*c, i))
        .collect();
    let group_count = comm_list.len() + 1;

    let palette_size = MAX_PALETTE.min(group_count);
    let color_of_group = |g: usize| -> String {
        let hue = ((g % palette_size) as f64 / palette_size as f64 * 360.0) as i32;
        format!("hsl({hue}, 70%, 60%)")
    };

    // Community centers on a golden-angle (sunflower) spiral: equal-area slots
    // scaled to the largest community, so discs never overlap and the layout
    // stays deterministic and compact at any community count. Nodes without a
    // community get one trailing slot.
    let uncategorized = rows.iter().filter(|(.., comm)| comm.is_none()).count();
    let group_total = comm_list.len() + 1;
    let radii: Vec<f64> = comm_list
        .iter()
        .map(|(_, n)| NODE_SPACING * (*n as f64).sqrt())
        .chain(std::iter::once(
            NODE_SPACING * (uncategorized as f64).sqrt(),
        ))
        .collect();
    let largest_radius = radii.iter().copied().fold(0.0_f64, f64::max);
    let center_step = 2.2 * largest_radius + COMMUNITY_GAP;

    let mut centers: Vec<(f64, f64)> = Vec::with_capacity(group_total);
    for (i, _) in radii.iter().enumerate() {
        let dist = center_step * (i as f64).sqrt();
        let theta = i as f64 * GOLDEN_ANGLE;
        centers.push((dist * theta.cos(), dist * theta.sin()));
    }

    let round1 = |v: f64| -> f64 { (v * 10.0).round() / 10.0 };

    // Bubbles: one per community, plus one for the uncategorized bucket.
    let mut communities: Vec<CommunityOut> = Vec::with_capacity(group_total);
    for (g, (c, count)) in comm_list.iter().enumerate() {
        communities.push(CommunityOut {
            id: Some(*c),
            label: label_of(c),
            size: *count,
            color: color_of_group(g),
            x: round1(centers[g].0),
            y: round1(centers[g].1),
        });
    }
    if uncategorized > 0 {
        let g = comm_list.len();
        communities.push(CommunityOut {
            id: None,
            label: "No community".to_string(),
            size: uncategorized,
            color: color_of_group(g),
            x: round1(centers[g].0),
            y: round1(centers[g].1),
        });
    }

    let mut nodes: Vec<NodeOut> = Vec::with_capacity(node_count);
    // Per-community node cursor, in size-rank order.
    let mut cursors: Vec<u64> = vec![0; group_total];
    for (id, label, ft, sf, line, comm) in &rows {
        let group = match comm {
            Some(c) => group_index.get(c).copied().unwrap_or(comm_list.len()),
            None => comm_list.len(),
        };
        // Nodes fan out on their own sunflower around the community center.
        let j = cursors[group];
        cursors[group] += 1;
        let dist = NODE_SPACING * (j as f64).sqrt();
        let theta = j as f64 * GOLDEN_ANGLE;
        let (cx, cy) = centers[group];
        nodes.push(NodeOut {
            id: id.clone(),
            label: label.clone(),
            file_type: ft.clone(),
            source_file: sf.clone(),
            source_line: *line,
            community: *comm,
            color: color_of_group(group),
            x: round1(cx + dist * theta.cos()),
            y: round1(cy + dist * theta.sin()),
            degree: degrees.get(id).copied().unwrap_or(0),
        });
    }

    // Center the whole layout on the origin.
    if !nodes.is_empty() {
        let bounds = |get: fn(&NodeOut) -> f64| -> (f64, f64) {
            let mut min = f64::INFINITY;
            let mut max = f64::NEG_INFINITY;
            for n in &nodes {
                let v = get(n);
                min = min.min(v);
                max = max.max(v);
            }
            (min, max)
        };
        let (min_x, max_x) = bounds(|n| n.x);
        let (min_y, max_y) = bounds(|n| n.y);
        let (ox, oy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
        for n in &mut nodes {
            n.x = round1(n.x - ox);
            n.y = round1(n.y - oy);
        }
    }

    let hyper_edges: Vec<HyperEdgeOut> = astria_build::hyperedges::load_all(db)?
        .into_iter()
        .map(|h| HyperEdgeOut {
            id: h.id,
            label: h.label,
            nodes: h.nodes,
        })
        .collect();

    Ok(Payload {
        meta: Meta {
            node_count,
            edge_count,
            community_count: comm_list.len(),
        },
        nodes,
        edges,
        communities,
        hyper_edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    #[test]
    fn exports_self_contained_bubble_viewer() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file, community) VALUES
              ('a', 'Alpha()', 'code', 'src/a.rs', 1),
              ('b', 'Beta()', 'code', 'src/b.rs', 1),
              ('c', 'Gamma()', 'code', 'src/c.py', 2);
            INSERT INTO edges (source, target, relation, confidence, source_file)
              VALUES ('a', 'b', 'calls', 'EXTRACTED', 'src/a.rs'),
                     ('a', 'c', 'calls', 'EXTRACTED', 'src/a.rs');
        ",
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("graph-view.html");
        export_html(&db, &out).unwrap();

        let html = std::fs::read_to_string(&out).unwrap();
        // The page must be self-contained: embedded data + one viewer bundle.
        assert!(html.contains("var DATA ="));
        // Nodes and bubbles carry coordinates and colors from Rust.
        assert!(html.contains(r#""x":"#));
        assert!(html.contains(r#""color":"#));
        // Edges carry their relation so the focus panel can name the links.
        assert!(html.contains(r#""relation":"calls""#));
        // Community bubbles are precomputed, with fallback labels.
        assert!(html.contains(r#""label":"Community 1""#));
        assert!(html.contains(r#""communityCount":2"#));
        // Node labels land in the embedded data.
        assert!(html.contains("Alpha()"));
        // The viewer bundle is the new canvas viewer (not vis-network).
        assert!(!html.contains("vis-network"));
    }

    #[test]
    fn escapes_script_breaking_sequences_in_data() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('a', '</script><script>alert(1)</script>', 'html', 'a<!--b.html');
        ",
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("graph-view.html");
        export_html(&db, &out).unwrap();

        let html = std::fs::read_to_string(&out).unwrap();
        assert!(html.contains(r#"\u003c/script"#));
        assert!(html.contains(r#"\u003c!--"#));
    }

    #[test]
    fn standard_mode_rejects_graphs_over_the_viz_cap() {
        let db = open_db_in_memory().unwrap();
        let mut batch = String::from("BEGIN;\n");
        for i in 0..(MAX_NODES_FOR_VIZ + 1) {
            let id = format!("n{i}");
            batch.push_str(&format!(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('{id}', 'N{i}()', 'code', 'f.rs');\n"
            ));
        }
        batch.push_str("COMMIT;");
        db.execute_batch(&batch).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let standard_err = export_html(&db, &dir.path().join("standard.html")).unwrap_err();
        assert!(standard_err.to_string().contains("exceeding"));

        let out = dir.path().join("large.html");
        export_html_with_mode(&db, &out, HtmlExportMode::Large).unwrap();
        let html = std::fs::read_to_string(&out).unwrap();
        assert!(html.contains(&format!(r#""nodeCount":{}"#, MAX_NODES_FOR_VIZ + 1)));
    }

    #[test]
    fn uncategorized_nodes_get_their_own_bubble() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('a', 'Alpha()', 'code', 'src/a.rs'),
              ('b', 'Beta()', 'code', 'src/b.rs'),
              ('c', 'Gamma()', 'code', 'src/c.py');
        ",
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("graph-view.html");
        export_html(&db, &out).unwrap();

        let html = std::fs::read_to_string(&out).unwrap();
        assert!(html.contains(r#""label":"No community""#));
        assert!(html.contains(r#""size":3"#));
    }

    #[test]
    fn viewer_bundle_never_builds_markup_from_labels() {
        // Labels come from repo content and LLM output; the viewer must only
        // reach them via canvas text or DOM textContent, never innerHTML.
        assert!(
            !VIEWER_JS.contains("innerHTML"),
            "viewer must not concatenate data into markup via innerHTML"
        );
        // And the bundle itself must not break out of its script tag.
        assert!(!VIEWER_JS.contains("</script"));
    }
}
