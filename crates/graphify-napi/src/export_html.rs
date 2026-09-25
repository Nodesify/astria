use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

const VIS_NETWORK_JS: &str = include_str!("assets/vis-network.min.js");
/// Maximum graph size accepted by the reference/standard HTML exporter.
pub const MAX_NODES_FOR_VIZ: usize = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlExportMode {
    Standard,
    Large,
}

impl HtmlExportMode {
    pub fn parse(value: &str) -> graphify_core::Result<Self> {
        match value {
            "standard" => Ok(Self::Standard),
            "large" => Ok(Self::Large),
            other => Err(graphify_core::GraphifyError::Graph(format!(
                "invalid HTML export mode '{other}'; expected 'standard' or 'large'"
            ))),
        }
    }
}

pub fn export_html(db: &Connection, out_path: &Path) -> graphify_core::Result<()> {
    export_html_with_mode(db, out_path, HtmlExportMode::Standard)
}

pub fn export_html_with_mode(
    db: &Connection,
    out_path: &Path,
    mode: HtmlExportMode,
) -> graphify_core::Result<()> {
    let node_count: usize = db.query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))?;
    if mode == HtmlExportMode::Standard && node_count > MAX_NODES_FOR_VIZ {
        return Err(graphify_core::GraphifyError::Graph(format!(
            "graph has {node_count} nodes, exceeding the standard HTML visualization limit of {MAX_NODES_FOR_VIZ}; rerun with --mode large"
        )));
    }
    export_html_impl(db, out_path, mode)
}

/// Distance between neighboring nodes inside a community, in px.
const NODE_SPACING: f64 = 38.0;
/// Extra gap between neighboring community centers, in px.
const COMMUNITY_GAP: f64 = 220.0;
/// Distinct hues in the palette; groups beyond this cycle through it.
const MAX_PALETTE: usize = 60;
/// Legend rows before the remaining communities collapse into one "... more" row.
const LEGEND_LIMIT: usize = 20;
/// Nodes visible on load; the rest appear via search or the "Show all" toggle.
const MAX_INITIAL_NODES: usize = 2500;
/// Edge arrows are dropped above this many edges (large draw cost per frame).
const ARROW_EDGE_LIMIT: usize = 5000;

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
    /// Part of the initially visible subset (top-degree nodes).
    key: bool,
    degree: i64,
}

#[derive(Serialize)]
struct EdgeOut {
    from: String,
    to: String,
}

#[derive(Serialize)]
struct LegendEntry {
    label: String,
    count: usize,
    color: String,
}

#[derive(Serialize)]
struct Meta {
    #[serde(rename = "nodeCount")]
    node_count: usize,
    #[serde(rename = "edgeCount")]
    edge_count: usize,
    #[serde(rename = "showArrows")]
    show_arrows: bool,
    #[serde(rename = "initialCount")]
    initial_count: usize,
    #[serde(rename = "legendRestCount")]
    legend_rest_count: usize,
}

#[derive(Serialize)]
struct Payload {
    nodes: Vec<NodeOut>,
    edges: Vec<EdgeOut>,
    #[serde(rename = "hyperedges")]
    hyper_edges: Vec<HyperEdgeOut>,
    legend: Vec<LegendEntry>,
    meta: Meta,
}

#[derive(Serialize)]
struct HyperEdgeOut {
    id: String,
    label: String,
    nodes: Vec<String>,
}

fn export_html_impl(
    db: &Connection,
    out_path: &Path,
    mode: HtmlExportMode,
) -> graphify_core::Result<()> {
    let large_mode = mode == HtmlExportMode::Large;
    let payload = build_payload(db, large_mode)?;

    let data_json = serde_json::to_string(&payload)?;
    // Keep the embedded JSON from terminating the surrounding <script> block
    // (\u003c is valid in both JSON and JS string literals).
    let data_json = data_json
        .replace("</script", "\\u003c/script")
        .replace("<!--", "\\u003c!--");

    let vis_js = VIS_NETWORK_JS.replace("</script", "\\u003c/script");

    let html = format!(
        r##"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>graphify &mdash; Knowledge Graph</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 0; padding: 0; background: #1a1a2e; color: #e0e0e0; }}
    #search {{ position: fixed; top: 12px; left: 12px; z-index: 100; }}
    #search input {{ width: 280px; padding: 8px 12px; border: 1px solid #444; border-radius: 6px; background: #16213e; color: #e0e0e0; font-size: 14px; }}
    #search input::placeholder {{ color: #888; }}
    #info {{ position: fixed; bottom: 12px; left: 12px; z-index: 100; background: #16213e; border: 1px solid #333; border-radius: 6px; padding: 10px 14px; max-width: 400px; font-size: 13px; display: none; }}
    #info .label {{ font-weight: bold; font-size: 15px; margin-bottom: 4px; }}
    #info .meta {{ color: #aaa; }}
    #legend {{ position: fixed; top: 12px; right: 12px; z-index: 100; background: #16213e; border: 1px solid #333; border-radius: 6px; padding: 10px 14px; font-size: 12px; max-height: 300px; overflow-y: auto; }}
    #legend h4 {{ margin: 0 0 6px 0; color: #ccc; }}
    #legend .item {{ display: flex; align-items: center; gap: 6px; margin: 2px 0; }}
    #legend .swatch {{ width: 14px; height: 14px; border-radius: 3px; }}
    #legend .muted {{ color: #777; margin-top: 6px; }}
    #status {{ position: fixed; bottom: 12px; right: 12px; z-index: 100; display: flex; align-items: center; gap: 10px; background: #16213e; border: 1px solid #333; border-radius: 6px; padding: 8px 12px; font-size: 12px; color: #aaa; }}
    #toggleView {{ background: #0f3460; color: #e0e0e0; border: 1px solid #444; border-radius: 4px; padding: 5px 10px; font-size: 12px; cursor: pointer; }}
    #toggleView:hover {{ background: #1a4a8a; }}
    #network {{ width: 100vw; height: 100vh; }}
  </style>
</head>
<body>
  <div id="search"><input type="text" id="searchInput" placeholder="Search nodes..." /></div>
  <div id="legend"></div>
  <div id="info"><div class="label" id="infoLabel"></div><div class="meta" id="infoMeta"></div></div>
  <div id="status"><span id="statusText"></span><button id="toggleView" type="button"></button></div>
  <div id="network"></div>
  <script>
    {VIS_JS}
  </script>
  <script>
    var DATA = {DATA_JSON};

    var container = document.getElementById('network');
    var nodes = new vis.DataSet(DATA.nodes);
    var edges = new vis.DataSet(DATA.edges);

    // Lowercased search corpus, computed once instead of per keystroke.
    var searchIndex = DATA.nodes.map(function(n) {{
      return (n.label + ' ' + (n.sourceFile || '')).toLowerCase();
    }});
    var showAll = false;
    var visibleIds = [];

    // Legend is precomputed (counts + capped palette) to keep the DOM small.
    (function() {{
      var html = '<h4>Communities</h4>';
      DATA.legend.forEach(function(e) {{
        html += '<div class="item"><div class="swatch" style="background:' + e.color + '"></div> ' + e.label + ' (' + e.count + ' nodes)</div>';
      }});
      if (DATA.meta.legendRestCount > 0) {{
        html += '<div class="muted">+ ' + DATA.meta.legendRestCount + ' more communities</div>';
      }}
      document.getElementById('legend').innerHTML = html;
    }})();

    // One batched visibility pass per change instead of per-node updates.
    function applyView() {{
      var q = document.getElementById('searchInput').value.trim().toLowerCase();
      var updates = [];
      visibleIds.length = 0;
      for (var i = 0; i < DATA.nodes.length; i++) {{
        var n = DATA.nodes[i];
        var matches = !q || searchIndex[i].indexOf(q) !== -1;
        // Searching surfaces every match; otherwise only key nodes (or all).
        var hidden = !matches || !(q || showAll || n.key);
        if (!!n.hidden !== hidden) {{
          n.hidden = hidden;
          updates.push({{ id: n.id, hidden: hidden }});
        }}
        if (!hidden) visibleIds.push(n.id);
      }}
      if (updates.length) nodes.update(updates);
      var toggle = document.getElementById('toggleView');
      toggle.textContent = showAll ? 'Show key nodes' : 'Show all ' + DATA.meta.nodeCount + ' nodes';
      document.getElementById('statusText').textContent =
        'Showing ' + visibleIds.length + ' of ' + DATA.meta.nodeCount + ' nodes';
    }}

    var searchDebounce = null;
    document.getElementById('searchInput').addEventListener('input', function() {{
      clearTimeout(searchDebounce);
      searchDebounce = setTimeout(applyView, 120);
    }});
    document.getElementById('toggleView').addEventListener('click', function() {{
      showAll = !showAll;
      applyView();
    }});
    applyView();

    var options = {{
      autoResize: true,
      nodes: {{ shape: 'dot', size: 12, font: {{ color: '#e0e0e0', size: 11 }}, borderWidth: 0, borderWidthSelected: 2 }},
      edges: {{
        color: {{ color: '#555', highlight: '#fff', hover: '#aaa' }},
        smooth: false,
        arrows: DATA.meta.showArrows ? {{ to: {{ enabled: true, scaleFactor: 0.4 }} }} : {{ to: {{ enabled: false }} }},
        selectionWidth: 1.5
      }},
      // Positions are precomputed, so physics (the multi-minute stabilization
      // on large graphs) stays off permanently.
      physics: {{ enabled: false }},
      interaction: {{ hover: true, tooltipDelay: 300, navigationButtons: true, keyboard: true }}
    }};

    var network = new vis.Network(container, {{ nodes: nodes, edges: edges }}, options);

    // Hyperedges as shaded convex hulls behind the nodes. Positions come from
    // the precomputed layout (DATA.nodes x/y) converted to DOM pixels via the
    // network's own transform, so hulls track pan/zoom for free.
    (function() {{
      var hyper = (DATA.hyperedges || []).map(function(h) {{
        var pts = [];
        (h.nodes || []).forEach(function(id) {{
          var n = DATA.nodes.find(function(x) {{ return x.id === id; }});
          if (n) pts.push({{ x: n.x, y: n.y }});
        }});
        return {{ label: h.label, pts: pts }};
      }}).filter(function(h) {{ return h.pts.length >= 3; }});
      if (!hyper.length) return;

      function drawHulls(ctx) {{
        hyper.forEach(function(h) {{
          var pts = h.pts.map(function(p) {{
            var dom = network.canvasToDOM({{ x: p.x, y: p.y }});
            return {{ x: dom.x, y: dom.y }};
          }});
          var cx = 0, cy = 0;
          pts.forEach(function(p) {{ cx += p.x; cy += p.y; }});
          cx /= pts.length; cy /= pts.length;
          ctx.save();
          ctx.globalAlpha = 0.10;
          ctx.fillStyle = '#6366f1';
          ctx.strokeStyle = '#6366f1';
          ctx.lineWidth = 2;
          ctx.beginPath();
          pts.forEach(function(p, i) {{
            var ex = cx + (p.x - cx) * 1.18, ey = cy + (p.y - cy) * 1.18;
            if (i === 0) ctx.moveTo(ex, ey); else ctx.lineTo(ex, ey);
          }});
          ctx.closePath();
          ctx.fill();
          ctx.globalAlpha = 0.35;
          ctx.stroke();
          ctx.restore();
        }});
      }}
      network.on('beforeDrawing', drawHulls);
    }})();

    if (visibleIds.length) {{
      network.fit({{ nodes: visibleIds, animation: false }});
    }}

    network.on('click', function(params) {{
      var info = document.getElementById('info');
      if (params.nodes.length > 0) {{
        var node = nodes.get(params.nodes[0]);
        document.getElementById('infoLabel').textContent = node.label;
        var meta = (node.sourceFile || '?') + (node.sourceLine ? ':' + node.sourceLine : '')
          + ' | ' + node.fileType
          + ' | ' + (node.community === null ? 'no community' : 'community ' + node.community)
          + ' | ' + node.degree + ' connections';
        document.getElementById('infoMeta').textContent = meta;
        info.style.display = 'block';
      }} else {{
        info.style.display = 'none';
      }}
    }});
  </script>
</body>
</html>"##,
        VIS_JS = vis_js,
        DATA_JSON = data_json,
    );

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, html)?;
    Ok(())
}

fn build_payload(db: &Connection, large_mode: bool) -> graphify_core::Result<Payload> {
    let mut degrees: HashMap<String, i64> = HashMap::new();
    let mut edges: Vec<EdgeOut> = Vec::new();
    {
        let mut stmt = db.prepare("SELECT source, target FROM edges")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let src: String = row.get(0)?;
            let tgt: String = row.get(1)?;
            *degrees.entry(src.clone()).or_insert(0) += 1;
            *degrees.entry(tgt.clone()).or_insert(0) += 1;
            edges.push(EdgeOut { from: src, to: tgt });
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

    // Initial view: top-degree nodes, deterministic tie-break by id.
    let mut by_degree: Vec<(String, i64)> = rows
        .iter()
        .map(|(id, ..)| {
            let d = degrees.get(id).copied().unwrap_or(0);
            (id.clone(), d)
        })
        .collect();
    by_degree.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let key_nodes: HashSet<String> = by_degree
        .into_iter()
        .take(if large_mode {
            MAX_INITIAL_NODES
        } else {
            node_count
        })
        .map(|(id, _)| id)
        .collect();

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
        let round1 = |v: f64| -> f64 { (v * 10.0).round() / 10.0 };
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
            key: key_nodes.contains(id),
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
            n.x = ((n.x - ox) * 10.0).round() / 10.0;
            n.y = ((n.y - oy) * 10.0).round() / 10.0;
        }
    }

    let legend: Vec<LegendEntry> = comm_list
        .iter()
        .enumerate()
        .take(LEGEND_LIMIT)
        .map(|(g, (c, count))| LegendEntry {
            label: format!("Community {c}"),
            count: *count,
            color: color_of_group(g),
        })
        .collect();
    let legend_rest_count: usize = comm_list.iter().skip(LEGEND_LIMIT).map(|(_, n)| n).sum();

    let hyper_edges: Vec<HyperEdgeOut> = graphify_build::hyperedges::load_all(db)?
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
            show_arrows: edge_count <= ARROW_EDGE_LIMIT,
            initial_count: key_nodes.len().min(node_count),
            legend_rest_count,
        },
        nodes,
        edges,
        hyper_edges,
        legend,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphify_core::db::open_db_in_memory;

    #[test]
    fn exports_precomputed_layout_without_physics() {
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
        // Physics must stay off: positions are precomputed.
        assert!(html.contains("physics: { enabled: false }"));
        // Nodes carry coordinates and colors from Rust.
        assert!(html.contains(r#""x":"#));
        assert!(html.contains(r#""color":"#));
        // Straight edges, arrows on for a small graph.
        assert!(html.contains("smooth: false"));
        assert!(html.contains(r#""showArrows":true"#));
        // Legend entries are precomputed.
        assert!(html.contains(r#""label":"Community 1""#));
        // Search matches by label.
        assert!(html.contains("Alpha()"));
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
    fn key_node_subset_is_capped() {
        let db = open_db_in_memory().unwrap();
        let mut batch = String::from("BEGIN;\n");
        for i in 0..(MAX_INITIAL_NODES + 100) {
            let id = format!("n{i}");
            batch.push_str(&format!(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('{id}', 'N{i}()', 'code', 'f.rs');\n"
            ));
            if i % 2 == 0 {
                batch.push_str(&format!(
                    "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('{id}', 'n0', 'calls', 'EXTRACTED', 'f.rs');\n"
                ));
            }
        }
        batch.push_str("COMMIT;");
        db.execute_batch(&batch).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("graph-view.html");
        export_html_with_mode(&db, &out, HtmlExportMode::Large).unwrap();

        let html = std::fs::read_to_string(&out).unwrap();
        let key_markers = html.matches(r#""key":true"#).count();
        assert_eq!(key_markers, MAX_INITIAL_NODES);
        assert!(html.contains(&format!(r#""initialCount":{MAX_INITIAL_NODES}"#)));
    }

    #[test]
    fn legend_caps_at_limit_with_rest_row() {
        let db = open_db_in_memory().unwrap();
        let mut batch = String::from("BEGIN;\n");
        for c in 0..(LEGEND_LIMIT + 15) {
            for i in 0..2 {
                batch.push_str(&format!(
                    "INSERT INTO nodes (id, label, file_type, source_file, community) VALUES ('n{c}_{i}', 'N()', 'code', 'f.rs', {c});\n"
                ));
            }
        }
        batch.push_str("COMMIT;");
        db.execute_batch(&batch).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("graph-view.html");
        export_html(&db, &out).unwrap();

        let html = std::fs::read_to_string(&out).unwrap();
        let legend_rows = html.matches(r#""label":"Community"#).count();
        assert_eq!(legend_rows, LEGEND_LIMIT);
        assert!(html.contains(r#""legendRestCount":30"#));
    }
}
