// Obsidian vault export: one markdown note per node with YAML frontmatter
// (astria/* + community tags) and [[wikilinks]] to neighbors, community
// overview notes (underscore prefix sorts them to the top), and a
// astria.canvas file (communities as colored groups, nodes as cards,
// edges between them). Ported from upstream astria export.py
// (to_obsidian + to_canvas) as a script generator — open the output
// directory as a vault in Obsidian.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use astria_core::Result;

/// Wikilinks listed per relation section of a node note.
const MAX_LINKS_PER_RELATION: usize = 50;
/// Member links listed in a community overview note.
const MAX_MEMBERS_PER_NOTE: usize = 200;
/// Obsidian canvas color codes cycled per community (red..purple).
const CANVAS_COLORS: [&str; 6] = ["1", "2", "3", "4", "5", "6"];
/// Canvas card geometry.
const CARD_W: i64 = 250;
const CARD_H: i64 = 60;
const CARD_GAP: i64 = 20;
const CARDS_PER_ROW: usize = 3;

struct NodeRow {
    label: String,
    file_type: String,
    source_file: String,
    community: Option<i64>,
}

struct EdgeRow {
    source: String,
    target: String,
    relation: String,
    confidence: String,
}

/// Obsidian note names may not contain `# ^ [ ] |` (link syntax) or the
/// usual Windows-illegal characters.
fn note_name(label: &str) -> String {
    let mut out: String = label
        .chars()
        .map(|c| match c {
            '#' | '^' | '[' | ']' | '|' | '<' | '>' | ':' | '"' | '/' | '\\' | '*' | '?' => '-',
            c if c.is_whitespace() => '_',
            c => c,
        })
        .collect();
    out = out
        .trim_matches(|c: char| c == '.' || c == '-' || c.is_whitespace())
        .to_string();
    if out.len() > 80 {
        out.truncate(
            out.char_indices()
                .take(80)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(80),
        );
    }
    if out.is_empty() {
        out.push_str("unnamed");
    }
    // Windows reserved device names (NUL, CON, COM1...) cannot be written
    // as files — a node labeled "NUL" would otherwise break the export.
    let stem_lower = out.to_lowercase();
    if matches!(
        stem_lower.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    ) {
        out.insert(0, '_');
    }
    out
}

/// Obsidian tag segments allow letters, digits, `_`, `-`, `/`.
fn tag_segment(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == '/' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unnamed".to_string()
    } else {
        cleaned
    }
}

fn conf_rank(conf: &str) -> u8 {
    match conf {
        "EXTRACTED" => 0,
        "INFERRED" => 1,
        _ => 2,
    }
}

struct Vault {
    nodes: HashMap<String, NodeRow>,
    edges: Vec<EdgeRow>,
    /// node id -> undirected degree
    degrees: HashMap<String, usize>,
    /// community id -> (label, cohesion)
    communities: Vec<(i64, String, Option<f64>)>,
    /// node id -> unique note stem
    note_stems: HashMap<String, String>,
    /// community label -> note stem
    community_stems: HashMap<String, String>,
}

impl Vault {
    fn load(db: &Connection) -> Result<Self> {
        let mut nodes = HashMap::new();
        {
            let mut stmt =
                db.prepare("SELECT id, label, file_type, source_file, community FROM nodes")?;
            let rows = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                Ok((
                    id.clone(),
                    NodeRow {
                        label: row.get(1)?,
                        file_type: row.get(2)?,
                        source_file: row.get(3)?,
                        community: row.get(4)?,
                    },
                ))
            })?;
            for (id, node) in rows.flatten() {
                nodes.insert(id, node);
            }
        }

        let mut edges = Vec::new();
        {
            let mut stmt = db.prepare("SELECT source, target, relation, confidence FROM edges")?;
            let rows = stmt.query_map([], |row| {
                Ok(EdgeRow {
                    source: row.get(0)?,
                    target: row.get(1)?,
                    relation: row.get(2)?,
                    confidence: row.get(3)?,
                })
            })?;
            for edge in rows.flatten() {
                edges.push(edge);
            }
        }

        let mut degrees: HashMap<String, usize> = HashMap::new();
        for edge in &edges {
            *degrees.entry(edge.source.clone()).or_insert(0) += 1;
            *degrees.entry(edge.target.clone()).or_insert(0) += 1;
        }

        let mut communities = Vec::new();
        {
            let mut stmt = db.prepare("SELECT id, label, cohesion FROM communities ORDER BY id")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<f64>>(2)?,
                ))
            })?;
            for row in rows.flatten() {
                communities.push(row);
            }
        }

        // Unique note stems (wikilinks must resolve to exactly one note).
        let mut note_stems = HashMap::new();
        let mut used: HashSet<String> = HashSet::new();
        let mut by_label: Vec<(&String, &String)> =
            nodes.iter().map(|(id, n)| (&n.label, id)).collect();
        by_label.sort(); // deterministic collision suffixes
        for (label, id) in by_label {
            let base = note_name(label);
            let stem = if used.insert(base.clone()) {
                base
            } else {
                (2..)
                    .map(|n| format!("{base}-{n}"))
                    .find(|c| used.insert(c.clone()))
                    .expect("numeric suffixes cannot collide forever")
            };
            note_stems.insert(id.clone(), stem);
        }

        let mut community_stems = HashMap::new();
        let mut used_c: HashSet<String> = HashSet::new();
        for (_, label, _) in &communities {
            let base = note_name(label);
            let stem = if used_c.insert(base.clone()) {
                base
            } else {
                (2..)
                    .map(|n| format!("{base}-{n}"))
                    .find(|c| used_c.insert(c.clone()))
                    .expect("numeric suffixes cannot collide forever")
            };
            community_stems.insert(label.clone(), stem);
        }

        Ok(Vault {
            nodes,
            edges,
            degrees,
            communities,
            note_stems,
            community_stems,
        })
    }

    fn community_label(&self, id: i64) -> String {
        self.communities
            .iter()
            .find(|(cid, _, _)| *cid == id)
            .map(|(_, label, _)| label.clone())
            .unwrap_or_else(|| format!("Community {id}"))
    }

    /// Dominant confidence across the node's edges (most, then best rank).
    fn dominant_confidence(&self, id: &str) -> &'static str {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for edge in &self.edges {
            if edge.source == id || edge.target == id {
                let conf = match edge.confidence.as_str() {
                    "EXTRACTED" | "INFERRED" | "AMBIGUOUS" => match edge.confidence.as_str() {
                        "EXTRACTED" => "EXTRACTED",
                        "INFERRED" => "INFERRED",
                        _ => "AMBIGUOUS",
                    },
                    _ => "EXTRACTED",
                };
                *counts.entry(conf).or_insert(0) += 1;
            }
        }
        counts
            .into_iter()
            .max_by_key(|(conf, n)| (*n, 3 - conf_rank(conf)))
            .map(|(conf, _)| conf)
            .unwrap_or("EXTRACTED")
    }
}

fn node_note(vault: &Vault, id: &str, node: &NodeRow) -> String {
    let mut lines = Vec::new();
    let ftype_tag = tag_segment(&format!("astria/{}", node.file_type));
    let conf_tag = format!("astria/{}", vault.dominant_confidence(id));
    let community = node
        .community
        .map(|cid| vault.community_label(cid))
        .unwrap_or_else(|| "none".to_string());
    let comm_tag = format!("community/{}", tag_segment(&community));
    lines.push("---".to_string());
    lines.push(format!("tags: [{ftype_tag}, {conf_tag}, {comm_tag}]"));
    lines.push(format!(
        "source: \"{}\"",
        node.source_file.replace('\\', "/").replace('"', "'")
    ));
    lines.push(format!(
        "degree: {}",
        vault.degrees.get(id).copied().unwrap_or(0)
    ));
    lines.push("---".to_string());
    lines.push(String::new());
    lines.push("## Connections".to_string());
    lines.push(String::new());

    // neighbors grouped by relation, parallel edges collapsed to the best
    // confidence, highest-degree neighbors first
    let mut by_relation: HashMap<&str, HashMap<&String, &str>> = HashMap::new();
    for edge in &vault.edges {
        let neighbor = if edge.source == id {
            &edge.target
        } else if edge.target == id {
            &edge.source
        } else {
            continue;
        };
        let entry = by_relation
            .entry(edge.relation.as_str())
            .or_default()
            .entry(neighbor)
            .or_insert("");
        if entry.is_empty() || conf_rank(edge.confidence.as_str()) < conf_rank(entry) {
            *entry = edge.confidence.as_str();
        }
    }

    if by_relation.is_empty() {
        lines.push("(no connections)".to_string());
    } else {
        let mut relations: Vec<String> = by_relation.keys().map(|k| k.to_string()).collect();
        relations.sort();
        for rel in relations {
            let mut targets: Vec<(&String, &str)> = by_relation
                .remove(rel.as_str())
                .unwrap_or_default()
                .into_iter()
                .collect();
            targets.sort_by(|a, b| {
                vault
                    .degrees
                    .get(a.0)
                    .copied()
                    .unwrap_or(0)
                    .cmp(&vault.degrees.get(b.0).copied().unwrap_or(0))
                    .then_with(|| a.0.cmp(b.0))
            });
            lines.push(format!("### {rel}"));
            lines.push(String::new());
            for (neighbor, conf) in targets.iter().take(MAX_LINKS_PER_RELATION) {
                let stem = vault.note_stems.get(*neighbor).cloned();
                let label = vault
                    .nodes
                    .get(*neighbor)
                    .map(|n| n.label.as_str())
                    .unwrap_or(neighbor.as_str());
                let link = match stem {
                    Some(stem) => format!("[[{stem}|{label}]]"),
                    None => label.to_string(),
                };
                lines.push(format!("- {link} `{conf}`"));
            }
            let remaining = targets.len().saturating_sub(MAX_LINKS_PER_RELATION);
            if remaining > 0 {
                lines.push(format!("- *... and {remaining} more*"));
            }
            lines.push(String::new());
        }
    }
    lines.join("\n")
}

fn community_note(
    vault: &Vault,
    label: &str,
    cohesion: Option<f64>,
    member_ids: &[&String],
) -> String {
    let mut lines = Vec::new();
    lines.push("---".to_string());
    let mut front = vec![format!("community/{}", tag_segment(label))];
    if let Some(cohesion) = cohesion {
        front.push(format!("cohesion: {cohesion:.2}"));
    }
    lines.push(front.join("\n"));
    lines.push("---".to_string());
    lines.push(String::new());
    lines.push(format!("# {label}"));
    lines.push(String::new());
    lines.push(format!("> {} nodes", member_ids.len()));
    lines.push(String::new());

    let mut ranked: Vec<&String> = member_ids.to_vec();
    ranked.sort_by(|a, b| {
        vault
            .degrees
            .get(*a)
            .copied()
            .unwrap_or(0)
            .cmp(&vault.degrees.get(*b).copied().unwrap_or(0))
            .then_with(|| a.cmp(b))
    });
    for id in ranked.iter().take(MAX_MEMBERS_PER_NOTE) {
        if let Some(node) = vault.nodes.get(*id) {
            let stem = vault
                .note_stems
                .get(*id)
                .cloned()
                .unwrap_or_else(|| "unnamed".to_string());
            lines.push(format!("- [[{stem}|{}]]", node.label));
        }
    }
    let remaining = member_ids.len().saturating_sub(MAX_MEMBERS_PER_NOTE);
    if remaining > 0 {
        lines.push(format!("- *... and {remaining} more nodes*"));
    }
    lines.join("\n")
}

/// Canvas JSON: communities as colored group rectangles in a grid, nodes as
/// cards inside, edges between cards.
fn canvas_json(vault: &Vault, members: &HashMap<i64, Vec<&String>>) -> String {
    let mut nodes_json: Vec<serde_json::Value> = Vec::new();
    let mut edges_json: Vec<serde_json::Value> = Vec::new();
    let mut card_ids: HashMap<&String, String> = HashMap::new();

    // grid geometry: uniform cell sized by the largest community box
    let mut boxes: Vec<(i64, i64)> = Vec::new(); // (w, h) per community
    for (cid, _, _) in &vault.communities {
        let n = members.get(cid).map(|m| m.len()).unwrap_or(0);
        let rows = n.div_ceil(CARDS_PER_ROW).max(1);
        boxes.push((
            (CARDS_PER_ROW as i64 * (CARD_W + CARD_GAP) + CARD_GAP).max(400),
            rows as i64 * (CARD_H + CARD_GAP) + 3 * CARD_GAP,
        ));
    }
    let cell_w = boxes.iter().map(|(w, _)| *w).max().unwrap_or(400);
    let cell_h = boxes.iter().map(|(_, h)| *h).max().unwrap_or(300);
    let cols = ((vault.communities.len() as f64).sqrt().ceil().max(1.0)) as i64;

    for (idx, (cid, label, _)) in vault.communities.iter().enumerate() {
        let gx = (idx as i64 % cols) * (cell_w + CARD_GAP * 2);
        let gy = (idx as i64 / cols) * (cell_h + CARD_GAP * 2);
        let (bw, bh) = boxes[idx];
        nodes_json.push(serde_json::json!({
            "id": format!("group{idx}"),
            "type": "group",
            "label": label,
            "x": gx,
            "y": gy,
            "width": bw,
            "height": bh,
            "color": CANVAS_COLORS[idx % CANVAS_COLORS.len()],
        }));

        if let Some(member_ids) = members.get(cid) {
            let mut ranked: Vec<&String> = member_ids.to_vec();
            ranked.sort_by(|a, b| {
                vault
                    .degrees
                    .get(*a)
                    .copied()
                    .unwrap_or(0)
                    .cmp(&vault.degrees.get(*b).copied().unwrap_or(0))
            });
            for (i, id) in ranked.iter().enumerate() {
                let card_x = gx + CARD_GAP + (i % CARDS_PER_ROW) as i64 * (CARD_W + CARD_GAP);
                let card_y = gy + 2 * CARD_GAP + (i / CARDS_PER_ROW) as i64 * (CARD_H + CARD_GAP);
                let card_id = format!("card{}", card_ids.len());
                let stem = vault
                    .note_stems
                    .get(*id)
                    .cloned()
                    .unwrap_or_else(|| "unnamed".to_string());
                nodes_json.push(serde_json::json!({
                    "id": card_id,
                    "type": "file",
                    "file": format!("{stem}.md"),
                    "x": card_x,
                    "y": card_y,
                    "width": CARD_W,
                    "height": CARD_H,
                }));
                card_ids.insert(*id, card_id);
            }
        }
    }

    for edge in &vault.edges {
        if let (Some(from), Some(to)) = (card_ids.get(&edge.source), card_ids.get(&edge.target)) {
            if from != to {
                edges_json.push(serde_json::json!({
                    "id": format!("edge{}", edges_json.len()),
                    "fromNode": from,
                    "fromSide": "right",
                    "toNode": to,
                    "toSide": "left",
                }));
            }
        }
    }

    serde_json::to_string(&serde_json::json!({
        "nodes": nodes_json,
        "edges": edges_json,
    }))
    .unwrap_or_else(|_| "{\"nodes\":[],\"edges\":[]}".to_string())
}

/// Write an Obsidian vault: per-node notes, community overview notes, and
/// astria.canvas. Returns the number of notes written (nodes + communities).
pub fn export_obsidian(db: &Connection, out_dir: &Path) -> Result<usize> {
    let vault = Vault::load(db)?;
    std::fs::create_dir_all(out_dir)?;

    let mut members: HashMap<i64, Vec<&String>> = HashMap::new();
    for (id, node) in &vault.nodes {
        if let Some(cid) = node.community {
            members.entry(cid).or_default().push(id);
        }
    }

    let mut count = 0usize;
    for (id, node) in &vault.nodes {
        let stem = vault
            .note_stems
            .get(id)
            .cloned()
            .unwrap_or_else(|| "unnamed".to_string());
        std::fs::write(
            out_dir.join(format!("{stem}.md")),
            node_note(&vault, id, node),
        )?;
        count += 1;
    }

    for (cid, label, cohesion) in &vault.communities {
        let member_ids = members.get(cid).cloned().unwrap_or_default();
        let stem = vault
            .community_stems
            .get(label)
            .cloned()
            .unwrap_or_else(|| note_name(label));
        std::fs::write(
            out_dir.join(format!("_COMMUNITY_{stem}.md")),
            community_note(&vault, label, *cohesion, &member_ids),
        )?;
        count += 1;
    }

    std::fs::write(
        out_dir.join("astria.canvas"),
        canvas_json(&vault, &members),
    )?;

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    fn seeded_db() -> Connection {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file, community) VALUES
              ('a', 'Alpha()', 'code', 'src/a.rs', 1),
              ('b', 'Beta()', 'code', 'src/b.rs', 1),
              ('c', 'Gamma()', 'document', 'docs/c.md', 2);
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('a', 'b', 'calls', 'EXTRACTED', 'src/a.rs'),
              ('b', 'a', 'calls', 'INFERRED', 'src/b.rs'),
              ('a', 'c', 'imports', 'AMBIGUOUS', 'src/a.rs');
            INSERT INTO communities (id, label, cohesion, size) VALUES
              (1, 'Core', 0.8, 2),
              (2, 'Docs', 0.4, 1);
            ",
        )
        .unwrap();
        db
    }

    #[test]
    fn writes_vault_notes_and_canvas() {
        let db = seeded_db();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("vault");
        let count = export_obsidian(&db, &out).unwrap();
        // 3 node notes + 2 community notes
        assert_eq!(count, 5);

        let alpha = std::fs::read_to_string(out.join("Alpha().md")).unwrap();
        assert!(alpha.starts_with("---"));
        assert!(alpha.contains("tags: [astria/code, astria/EXTRACTED, community/Core]"));
        assert!(alpha.contains("### calls"));
        // duplicate calls edges collapse; wikilink targets the Beta note
        assert!(alpha.contains("[[Beta()|Beta()]] `EXTRACTED`"));
        let calls_block = alpha.split("### calls").nth(1).unwrap();
        assert_eq!(
            calls_block.matches("- [[Beta()]]").count() + calls_block.matches("[[Beta()|").count(),
            1
        );

        let core = std::fs::read_to_string(out.join("_COMMUNITY_Core.md")).unwrap();
        assert!(core.contains("# Core"));
        assert!(core.contains("[[Alpha()|Alpha()]]"));

        let canvas = std::fs::read_to_string(out.join("astria.canvas")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&canvas).unwrap();
        let nodes = parsed["nodes"].as_array().unwrap();
        let groups = nodes.iter().filter(|n| n["type"] == "group").count();
        let cards = nodes.iter().filter(|n| n["type"] == "file").count();
        assert_eq!(groups, 2);
        assert_eq!(cards, 3);
        let edges = parsed["edges"].as_array().unwrap();
        assert_eq!(edges.len(), 3);
    }

    #[test]
    fn note_names_strip_link_syntax_chars() {
        assert_eq!(note_name("weird|label#1"), "weird-label-1");
        assert_eq!(note_name("..."), "unnamed");
        assert_eq!(note_name("NUL"), "_NUL");
    }
}
