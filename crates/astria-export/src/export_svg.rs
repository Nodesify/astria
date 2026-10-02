// export_svg: static SVG rendering of the graph — a physics-free,
// deterministic community-arc layout. Communities sit on a ring, their
// members on small arcs around each center; edge lines and hub labels
// complete the picture. Output embeds anywhere (Notion, GitHub README),
// which the HTML viewer cannot.

use rusqlite::Connection;
use std::collections::HashMap;
use std::io::Write;

/// The layout caps: beyond these the export degrades gracefully instead of
/// producing a file no viewer can open.
const MAX_NODES: usize = 2000;
const MAX_EDGES: usize = 6000;
const MAX_LABELS: usize = 30;
const CANVAS: f64 = 1700.0;
const RING_RADIUS: f64 = 560.0;

pub struct SvgCounts {
    pub nodes: usize,
    pub edges: usize,
    pub communities: usize,
    pub truncated: bool,
}

struct SvgNode {
    id: String,
    label: String,
    community: i64,
    degree: usize,
}

/// XML-escape a text node or attribute value.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn export_svg(db: &Connection, out_path: &std::path::Path) -> astria_core::Result<SvgCounts> {
    // Nodes, id-sorted for a deterministic layout; degrees come from a
    // plain group-by below.
    let mut nodes: Vec<SvgNode> = {
        let mut stmt =
            db.prepare("SELECT id, label, COALESCE(community, -1) FROM nodes ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        rows.filter_map(|r| r.ok())
            .map(|(id, label, community)| SvgNode {
                id,
                label,
                community,
                degree: 0,
            })
            .collect()
    };
    {
        let mut stmt = db.prepare(
            "SELECT source, COUNT(*) FROM edges GROUP BY source
             UNION ALL
             SELECT target, COUNT(*) FROM edges GROUP BY target",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut degrees: HashMap<String, usize> = HashMap::new();
        for (id, cnt) in rows.filter_map(|r| r.ok()) {
            *degrees.entry(id).or_insert(0) += cnt as usize;
        }
        for node in &mut nodes {
            node.degree = degrees.get(&node.id).copied().unwrap_or(0);
        }
    }
    nodes.sort_by(|a, b| b.degree.cmp(&a.degree).then_with(|| a.id.cmp(&b.id)));
    let truncated = nodes.len() > MAX_NODES;
    nodes.truncate(MAX_NODES);
    // Re-sort by id for deterministic layout order.
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let drawn: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();

    // Edges among drawn nodes, capped.
    let mut edges: Vec<(String, String)> = {
        let mut stmt = db.prepare("SELECT source, target FROM edges ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.filter_map(|r| r.ok())
            .filter(|(s, t)| drawn.contains(s.as_str()) && drawn.contains(t.as_str()))
            .collect()
    };
    let edges_truncated = edges.len() > MAX_EDGES;
    edges.truncate(MAX_EDGES);

    // Community labels for the ring captions.
    let community_labels: HashMap<i64, String> = db
        .prepare("SELECT id, label FROM communities")
        .map(|mut stmt| {
            stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    // Group members by community (stable: nodes are id-sorted).
    let mut groups: Vec<(i64, Vec<&SvgNode>)> = Vec::new();
    {
        let mut index: HashMap<i64, usize> = HashMap::new();
        for node in &nodes {
            let idx = match index.get(&node.community) {
                Some(&i) => i,
                None => {
                    groups.push((node.community, Vec::new()));
                    index.insert(node.community, groups.len() - 1);
                    groups.len() - 1
                }
            };
            groups[idx].1.push(node);
        }
    }
    // Largest groups first around the ring.
    groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));

    // Layout: each community center on the main ring; members on an arc
    // around it, spread by golden-angle steps for even density.
    let group_count = groups.len().max(1);
    let mut positions: HashMap<&str, (f64, f64)> = HashMap::with_capacity(nodes.len());
    let mut group_caps: Vec<(f64, f64, String)> = Vec::new();
    let golden = 2.399963229728653_f64;
    for (gi, (community_id, members)) in groups.iter().enumerate() {
        let group_angle = 2.0 * std::f64::consts::PI * (gi as f64) / (group_count as f64);
        let cx = RING_RADIUS * group_angle.cos();
        let cy = RING_RADIUS * group_angle.sin();
        let spread = 26.0 + 11.0 * (members.len() as f64).sqrt();
        for (mi, node) in members.iter().enumerate() {
            let angle = golden * ((mi + 1) as f64) + group_angle;
            let radius = spread * ((mi as f64) + 0.5).sqrt() / (members.len() as f64).sqrt();
            let x = cx + radius * angle.cos();
            let y = cy + radius * angle.sin();
            positions.insert(node.id.as_str(), (x, y));
        }
        let caption = community_labels
            .get(community_id)
            .cloned()
            .unwrap_or_else(|| "unassigned".to_string());
        group_caps.push((cx, cy, caption));
    }

    // Hubs get labels: top MAX_LABELS by degree.
    let mut labeled: Vec<&SvgNode> = nodes.iter().collect();
    labeled.sort_by(|a, b| b.degree.cmp(&a.degree).then_with(|| a.id.cmp(&b.id)));
    labeled.truncate(MAX_LABELS);

    // Render.
    let half = CANVAS / 2.0;
    let neg_half = -half;
    let mut svg = String::with_capacity(1 << 20);
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{CANVAS}\" height=\"{CANVAS}\" viewBox=\"{neg_half} {neg_half} {CANVAS} {CANVAS}\" font-family=\"ui-sans-serif, system-ui, sans-serif\">\n"
    ));
    svg.push_str(&format!(
        "<rect x=\"{neg_half}\" y=\"{neg_half}\" width=\"{CANVAS}\" height=\"{CANVAS}\" fill=\"#0f1117\"/>\n"
    ));
    svg.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" fill=\"#e6e6e6\" font-size=\"20\">astria graph — {} nodes, {} edges, {} communities{}</text>\n",
        neg_half + 24.0,
        neg_half + 36.0,
        nodes.len(),
        edges.len(),
        group_count,
        if truncated || edges_truncated { " (capped)" } else { "" }
    ));

    // Community captions.
    for (cx, cy, caption) in &group_caps {
        let lx = cx * 1.22;
        let ly = cy * 1.22;
        svg.push_str(&format!(
            "<text x=\"{lx:.1}\" y=\"{ly:.1}\" fill=\"#9aa4b2\" font-size=\"15\" text-anchor=\"middle\">{}</text>\n",
            escape(caption)
        ));
    }

    // Edges first (under the nodes).
    svg.push_str("<g stroke=\"#7a8699\" stroke-opacity=\"0.14\" stroke-width=\"0.7\">\n");
    for (s, t) in &edges {
        let (Some((x1, y1)), Some((x2, y2))) =
            (positions.get(s.as_str()), positions.get(t.as_str()))
        else {
            continue;
        };
        svg.push_str(&format!(
            "<line x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\"/>\n"
        ));
    }
    svg.push_str("</g>\n");

    // Nodes.
    svg.push_str("<g stroke=\"#0f1117\" stroke-width=\"1\">\n");
    for node in &nodes {
        let Some((x, y)) = positions.get(node.id.as_str()) else {
            continue;
        };
        let hue = (node.community as f64 * 137.508) % 360.0;
        let radius = (3.0 + 2.0 * (1.0 + node.degree as f64).log2()).clamp(3.0, 13.0);
        svg.push_str(&format!(
            "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"{radius:.1}\" fill=\"hsl({hue:.0},58%,62%)\"/>\n"
        ));
    }
    svg.push_str("</g>\n");

    // Hub labels.
    svg.push_str("<g fill=\"#dfe3ea\" font-size=\"11\">\n");
    for node in &labeled {
        let Some((x, y)) = positions.get(node.id.as_str()) else {
            continue;
        };
        svg.push_str(&format!(
            "<text x=\"{x:.1}\" y=\"{y:.1}\" text-anchor=\"middle\" dy=\"-10\">{}</text>\n",
            escape(&node.label)
        ));
    }
    svg.push_str("</g>\n</svg>\n");

    let mut file = std::fs::File::create(out_path)?;
    file.write_all(svg.as_bytes())?;

    Ok(SvgCounts {
        nodes: nodes.len(),
        edges: edges.len(),
        communities: group_count,
        truncated: truncated || edges_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    fn seed(db: &Connection) {
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, community) VALUES
                ('a1', 'auth_login()', 'code', 'src/auth.rs', 0),
                ('a2', 'auth_check<b>', 'code', 'src/auth.rs', 0),
                ('b1', 'db_query()', 'code', 'src/db.rs', 1),
                ('b2', 'db_connect()', 'code', 'src/db.rs', 1),
                ('c1', 'misc()', 'code', 'src/misc.rs', NULL);
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('a1', 'a2', 'calls', 'EXTRACTED', 'src/auth.rs'),
                ('b1', 'b2', 'calls', 'EXTRACTED', 'src/db.rs'),
                ('a2', 'b1', 'calls', 'EXTRACTED', 'src/auth.rs');
             INSERT INTO communities (id, label, size) VALUES (0, 'Auth', 2), (1, 'Storage', 2);",
        )
        .unwrap();
    }

    #[test]
    fn svg_is_valid_escaped_and_deterministic() {
        let db = open_db_in_memory().unwrap();
        seed(&db);
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("graph.svg");

        let counts = export_svg(&db, &out).unwrap();
        assert_eq!(counts.nodes, 5);
        assert_eq!(counts.edges, 3);
        assert_eq!(counts.communities, 3, "two labeled + one unassigned group");
        assert!(!counts.truncated);

        let first = std::fs::read_to_string(&out).unwrap();
        assert!(first.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(first.ends_with("</svg>\n"));
        // Labels are XML-escaped, never raw markup.
        assert!(first.contains("auth_check&lt;b&gt;"));
        assert!(!first.contains("auth_check<b>"));
        // Community captions carry labels.
        assert!(first.contains("Auth"));
        assert!(first.contains("unassigned"));

        // Byte-identical on a second run: no HashMap-order leakage.
        export_svg(&db, &out).unwrap();
        let second = std::fs::read_to_string(&out).unwrap();
        assert_eq!(first, second, "SVG export must be deterministic");
    }

    #[test]
    fn empty_graph_renders_a_valid_frame() {
        let db = open_db_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("empty.svg");
        let counts = export_svg(&db, &out).unwrap();
        assert_eq!(counts.nodes, 0);
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.starts_with("<svg"));
        assert!(text.contains("0 nodes"));
    }
}
