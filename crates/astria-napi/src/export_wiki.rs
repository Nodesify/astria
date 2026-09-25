// Wiki export: Wikipedia-style markdown articles from the knowledge graph.
// Writes an agent-crawlable wiki — index.md plus one article per community
// and per god node — with relative markdown links so any agent (or GitHub,
// or Obsidian) can navigate it without the CLI. Ported from upstream
// astria wiki.py, adapted to the SQLite schema and confidence scores.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use astria_analyze::{analyze, NodeAnalysis, SurprisingEdge};
use astria_core::Result;

/// Key Concepts listed per community article before the "... and N more" line.
/// (The CLI default is applied in lib.rs; tests pass their own value.)
const MAX_CROSS_LINKS: usize = 12;
/// Source files listed per community article.
const MAX_SOURCE_FILES: usize = 20;
/// Neighbors listed per relation section of a god node article.
const MAX_NEIGHBORS_PER_RELATION: usize = 20;
/// Surprising connections listed on the index.
const MAX_SURPRISING_INDEX: usize = 10;
/// Cap article filename length (labels can be long signatures).
const MAX_SLUG_LEN: usize = 80;

struct NodeRow {
    label: String,
    source_file: String,
    community: Option<i64>,
    signature: Option<String>,
}

struct EdgeRow {
    source: String,
    target: String,
    relation: String,
    confidence: String,
}

/// Replace characters that are hostile to file names or URLs. Windows
/// forbids `<>:"/\|?*`; leading/trailing dots, spaces, and dashes also
/// break tools (a leading dash reads as a flag).
fn slug(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            c if c.is_whitespace() => '_',
            c => c,
        })
        .collect();
    out = out
        .trim_matches(|c: char| c == '.' || c == '-' || c.is_whitespace())
        .to_string();
    if out.len() > MAX_SLUG_LEN {
        out.truncate(
            out.char_indices()
                .take(MAX_SLUG_LEN)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(MAX_SLUG_LEN),
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

/// Assign `base` a unique file stem, appending -2, -3, ... on collision.
fn unique_slug(base: String, used: &mut HashSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }
    for n in 2.. {
        let candidate = format!("{base}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

/// Escape `[` and `]` so labels cannot break markdown link syntax.
fn link_text(label: &str) -> String {
    label.replace('[', "\\[").replace(']', "\\]")
}

/// Strip the repo root prefix from a stored source path so wiki articles
/// show root-relative paths (agent-facing output never leaks the build
/// machine's absolute directories). Falls back to the raw path.
fn root_relative(path: &str, root: Option<&Path>) -> String {
    let normalized = path.replace('\\', "/");
    if let Some(root) = root {
        let mut root_str = root.to_string_lossy().replace('\\', "/");
        if let Some(stripped) = root_str.strip_prefix("//?/") {
            root_str = stripped.to_string();
        }
        let prefix = format!("{}/", root_str.trim_end_matches('/'));
        if let Some(rest) = normalized.strip_prefix(&prefix) {
            return rest.to_string();
        }
    }
    normalized
}

/// EXTRACTED beats INFERRED beats anything else when collapsing duplicate
/// edges to the same neighbor.
fn conf_rank(conf: &str) -> u8 {
    match conf {
        "EXTRACTED" => 0,
        "INFERRED" => 1,
        _ => 2,
    }
}

fn md_link(text: &str, href: &str) -> String {
    format!("[{}]({})", link_text(text), href)
}

struct Wiki {
    nodes: HashMap<String, NodeRow>,
    edges: Vec<EdgeRow>,
    /// undirected degree per node id (in + out)
    degrees: HashMap<String, usize>,
    /// community id -> (label, cohesion)
    communities: Vec<(i64, String, Option<f64>)>,
    god_nodes: Vec<NodeAnalysis>,
    surprising: Vec<SurprisingEdge>,
    /// community label -> article stem under communities/
    community_stems: HashMap<String, String>,
    /// god node label -> article stem under nodes/
    node_stems: HashMap<String, String>,
    /// repo root for root-relative source paths
    root: Option<std::path::PathBuf>,
}

impl Wiki {
    fn load(db: &Connection, root: Option<&Path>) -> Result<Self> {
        let mut nodes = HashMap::new();
        {
            let mut stmt =
                db.prepare("SELECT id, label, source_file, community, signature FROM nodes")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    NodeRow {
                        label: row.get(1)?,
                        source_file: row.get(2)?,
                        community: row.get(3)?,
                        signature: row.get(4)?,
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

        let analysis = analyze(db)?;

        // Article stems must not collide within each directory.
        let mut community_stems = HashMap::new();
        let mut used_community: HashSet<String> = HashSet::new();
        for (_, label, _) in &communities {
            let stem = unique_slug(slug(label), &mut used_community);
            community_stems.insert(label.clone(), stem);
        }
        let mut node_stems = HashMap::new();
        let mut used_nodes: HashSet<String> = HashSet::new();
        for node in &analysis.god_nodes {
            if !node_stems.contains_key(&node.label) {
                let stem = unique_slug(slug(&node.label), &mut used_nodes);
                node_stems.insert(node.label.clone(), stem);
            }
        }

        Ok(Wiki {
            nodes,
            edges,
            degrees,
            communities,
            god_nodes: analysis.god_nodes,
            surprising: analysis.surprising_connections,
            community_stems,
            node_stems,
            root: root.map(|r| r.to_path_buf()),
        })
    }

    fn rel(&self, path: &str) -> String {
        root_relative(path, self.root.as_deref())
    }

    fn community_label(&self, id: i64) -> String {
        self.communities
            .iter()
            .find(|(cid, _, _)| *cid == id)
            .map(|(_, label, _)| label.clone())
            .unwrap_or_else(|| format!("Community {id}"))
    }

    /// Link to a community article from any article in the wiki. Falls back
    /// to plain text when the community has no article (unknown id).
    fn community_link(&self, id: i64, prefix: &str) -> String {
        let label = self.community_label(id);
        match self.community_stems.get(&label) {
            Some(stem) => md_link(&label, &format!("{prefix}communities/{stem}.md")),
            None => link_text(&label),
        }
    }

    /// Link to a node article when one exists (god nodes only), else the
    /// bare label in backticks.
    fn node_link(&self, label: &str, prefix: &str) -> String {
        match self.node_stems.get(label) {
            Some(stem) => md_link(label, &format!("{prefix}nodes/{stem}.md")),
            None => format!("`{}`", label.replace('`', "'")),
        }
    }
}

fn community_article(
    wiki: &Wiki,
    label: &str,
    cohesion: Option<f64>,
    member_ids: &[&String],
    max_key_nodes: usize,
) -> String {
    let member_set: HashSet<&String> = member_ids.iter().copied().collect();

    // Top members by undirected degree, deterministic order on ties.
    let mut ranked: Vec<&String> = member_ids.to_vec();
    ranked.sort_by(|a, b| {
        wiki.degrees
            .get(*a)
            .copied()
            .unwrap_or(0)
            .cmp(&wiki.degrees.get(*b).copied().unwrap_or(0))
            .then_with(|| a.cmp(b))
    });

    // Cross-community edge counts and confidence audit, one pass over edges
    // touching this community (each edge counted once, like nx undirected).
    let mut cross_counts: HashMap<i64, usize> = HashMap::new();
    let mut conf_counts: HashMap<&str, usize> = HashMap::new();
    for edge in &wiki.edges {
        let src_in = member_set.contains(&edge.source);
        let tgt_in = member_set.contains(&edge.target);
        if src_in || tgt_in {
            *conf_counts
                .entry(match edge.confidence.as_str() {
                    c @ ("EXTRACTED" | "INFERRED" | "AMBIGUOUS") => c,
                    _ => "EXTRACTED",
                })
                .or_insert(0) += 1;
        }
        if src_in != tgt_in {
            let other = if src_in {
                wiki.nodes.get(&edge.target).and_then(|n| n.community)
            } else {
                wiki.nodes.get(&edge.source).and_then(|n| n.community)
            };
            if let Some(other) = other {
                *cross_counts.entry(other).or_insert(0) += 1;
            }
        }
    }

    let mut sources: Vec<String> = member_ids
        .iter()
        .filter_map(|id| wiki.nodes.get(*id))
        .map(|n| wiki.rel(&n.source_file))
        .filter(|s| !s.is_empty())
        .collect();
    sources.sort_unstable();
    sources.dedup();

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("# {}", link_text(label)));
    lines.push(String::new());

    let mut meta_parts = vec![format!("{} nodes", member_ids.len())];
    if let Some(cohesion) = cohesion {
        meta_parts.push(format!("cohesion {cohesion:.2}"));
    }
    lines.push(format!("> {}", meta_parts.join(" · ")));
    lines.push(String::new());

    lines.push("## Key Concepts".into());
    lines.push(String::new());
    for id in ranked.iter().take(max_key_nodes) {
        if let Some(node) = wiki.nodes.get(*id) {
            let degree = wiki.degrees.get(*id).copied().unwrap_or(0);
            let src = if node.source_file.is_empty() {
                String::new()
            } else {
                format!(" — `{}`", wiki.rel(&node.source_file))
            };
            lines.push(format!(
                "- **{}** ({} connections){}",
                link_text(&node.label),
                degree,
                src
            ));
        }
    }
    let remaining = member_ids.len().saturating_sub(max_key_nodes);
    if remaining > 0 {
        lines.push(format!(
            "- *... and {remaining} more nodes in this community*"
        ));
    }
    lines.push(String::new());

    lines.push("## Relationships".into());
    lines.push(String::new());
    let mut cross: Vec<(i64, usize)> = cross_counts.into_iter().collect();
    cross.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if cross.is_empty() {
        lines.push("- No strong cross-community connections detected".into());
    } else {
        for (other_cid, count) in cross.into_iter().take(MAX_CROSS_LINKS) {
            lines.push(format!(
                "- {} ({} shared connections)",
                wiki.community_link(other_cid, "../"),
                count
            ));
        }
    }
    lines.push(String::new());

    if !sources.is_empty() {
        lines.push("## Source Files".into());
        lines.push(String::new());
        for src in sources.iter().take(MAX_SOURCE_FILES) {
            lines.push(format!("- `{src}`"));
        }
        let remaining = sources.len().saturating_sub(MAX_SOURCE_FILES);
        if remaining > 0 {
            lines.push(format!("- *... and {remaining} more files*"));
        }
        lines.push(String::new());
    }

    lines.push("## Audit Trail".into());
    lines.push(String::new());
    let total: usize = conf_counts.values().sum();
    for conf in ["EXTRACTED", "INFERRED", "AMBIGUOUS"] {
        let n = conf_counts.get(conf).copied().unwrap_or(0);
        let pct = if total == 0 {
            0
        } else {
            ((n as f64 / total as f64) * 100.0).round() as usize
        };
        lines.push(format!("- {conf}: {n} ({pct}%)"));
    }
    lines.push(String::new());

    lines.push("---".into());
    lines.push(String::new());
    lines.push(format!(
        "*Part of the astria knowledge wiki. See {} to navigate.*",
        md_link("index", "../index.md")
    ));
    lines.join("\n")
}

fn god_node_article(wiki: &Wiki, node: &NodeAnalysis) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("# {}", link_text(&node.label)));
    lines.push(String::new());

    let src = wiki
        .nodes
        .get(&node.id)
        .map(|n| wiki.rel(&n.source_file))
        .unwrap_or_default();
    lines.push(format!(
        "> God node · {} connections · `{}`",
        node.degree, src
    ));
    lines.push(String::new());

    if let Some(cid) = node.community.map(|c| c as i64) {
        lines.push(format!(
            "**Community:** {}",
            wiki.community_link(cid, "../")
        ));
        lines.push(String::new());
    }

    if let Some(signature) = wiki
        .nodes
        .get(&node.id)
        .and_then(|n| n.signature.as_ref())
        .filter(|s| !s.is_empty())
    {
        lines.push("## Signature".into());
        lines.push(String::new());
        lines.push("```".into());
        lines.push(signature.clone());
        lines.push("```".into());
        lines.push(String::new());
    }

    // Neighbors of both directions, grouped by relation, highest degree
    // first within each group. Parallel edges to the same neighbor
    // (INFERRED call duplicates) collapse to the best confidence.
    let mut by_relation: HashMap<&str, HashMap<&String, &str>> = HashMap::new();
    for edge in &wiki.edges {
        let neighbor = if edge.source == node.id {
            &edge.target
        } else if edge.target == node.id {
            &edge.source
        } else {
            continue;
        };
        let entry = by_relation
            .entry(edge.relation.as_str())
            .or_default()
            .entry(neighbor)
            .or_insert("");
        if conf_rank(edge.confidence.as_str()) < conf_rank(entry) || entry.is_empty() {
            *entry = edge.confidence.as_str();
        }
    }

    if !by_relation.is_empty() {
        lines.push("## Connections by Relation".into());
        lines.push(String::new());
        let mut relations: Vec<String> = by_relation.keys().map(|k| k.to_string()).collect();
        relations.sort();
        for rel in relations {
            let mut targets: Vec<(&String, &str)> = by_relation
                .remove(rel.as_str())
                .unwrap_or_default()
                .into_iter()
                .collect();
            targets.sort_by(|a, b| {
                wiki.degrees
                    .get(a.0)
                    .copied()
                    .unwrap_or(0)
                    .cmp(&wiki.degrees.get(b.0).copied().unwrap_or(0))
                    .then_with(|| a.0.cmp(b.0))
            });
            lines.push(format!("### {rel}"));
            lines.push(String::new());
            for (neighbor, conf) in targets.iter().take(MAX_NEIGHBORS_PER_RELATION) {
                let label = wiki
                    .nodes
                    .get(*neighbor)
                    .map(|n| n.label.clone())
                    .unwrap_or_else(|| neighbor.to_string());
                let conf_tag = if conf.is_empty() {
                    String::new()
                } else {
                    format!(" `{conf}`")
                };
                lines.push(format!("- {}{conf_tag}", wiki.node_link(&label, "../")));
            }
            let remaining = targets.len().saturating_sub(MAX_NEIGHBORS_PER_RELATION);
            if remaining > 0 {
                lines.push(format!("- *... and {remaining} more*"));
            }
            lines.push(String::new());
        }
    }

    lines.push("---".into());
    lines.push(String::new());
    lines.push(format!(
        "*Part of the astria knowledge wiki. See {} to navigate.*",
        md_link("index", "../index.md")
    ));
    lines.join("\n")
}

fn index_md(
    wiki: &Wiki,
    member_counts: &HashMap<i64, usize>,
    total_edges: usize,
    hyperedges: &[astria_build::hyperedges::HyperEdge],
) -> String {
    let total_nodes = wiki.nodes.len();
    let mut lines: Vec<String> = vec![
        "# Knowledge Graph Index".into(),
        String::new(),
        "> Auto-generated by astria. Start here — read community articles for context, then drill into god nodes for detail.".into(),
        String::new(),
        format!(
            "**{total_nodes} nodes · {total_edges} edges · {} communities**",
            wiki.communities.len()
        ),
        String::new(),
        "---".into(),
        String::new(),
        "## Communities".into(),
        "(sorted by size, largest first)".into(),
        String::new(),
    ];

    let mut ordered: Vec<&(i64, String, Option<f64>)> = wiki.communities.iter().collect();
    ordered.sort_by(|a, b| {
        member_counts
            .get(&b.0)
            .copied()
            .unwrap_or(0)
            .cmp(&member_counts.get(&a.0).copied().unwrap_or(0))
            .then_with(|| a.0.cmp(&b.0))
    });
    for (cid, _, _) in ordered {
        let size = member_counts.get(cid).copied().unwrap_or(0);
        lines.push(format!(
            "- {} — {size} nodes",
            wiki.community_link(*cid, "")
        ));
    }
    lines.push(String::new());

    if !wiki.god_nodes.is_empty() {
        lines.push("## God Nodes".into());
        lines.push("(most connected concepts — the load-bearing abstractions)".into());
        lines.push(String::new());
        for node in &wiki.god_nodes {
            lines.push(format!(
                "- {} — {} connections",
                wiki.node_link(&node.label, ""),
                node.degree
            ));
        }
        lines.push(String::new());
    }

    if !hyperedges.is_empty() {
        lines.push("## Hyperedges".into());
        lines.push("(group relationships — N-ary, not pairwise)".into());
        lines.push(String::new());
        for h in hyperedges.iter().take(MAX_SURPRISING_INDEX) {
            lines.push(format!(
                "- **{}** — {} members [{}] `{}`",
                h.label,
                h.nodes.len(),
                h.confidence,
                h.id
            ));
        }
        lines.push(String::new());
    }

    if !wiki.surprising.is_empty() {
        lines.push("## Surprising Connections".into());
        lines.push("(cross-community edges ranked by novelty)".into());
        lines.push(String::new());
        for edge in wiki.surprising.iter().take(MAX_SURPRISING_INDEX) {
            let src = wiki.node_link(&edge.source_label, "");
            let tgt = wiki.node_link(&edge.target_label, "");
            lines.push(format!("- {src} --{}--> {tgt}", edge.relation));
        }
        lines.push(String::new());
    }

    lines.push("---".into());
    lines.push(String::new());
    lines.push("*Generated by astria*".into());
    lines.join("\n")
}

/// Generate a Wikipedia-style wiki from the graph database into `out_dir`.
///
/// Writes:
///   - `index.md`            — agent entry point, catalog of all articles
///   - `communities/<..>.md` — one article per community
///   - `nodes/<..>.md`       — one article per god node
///
/// Pass `root` to render source paths root-relative. Returns the number of
/// articles written (excluding index.md).
pub fn export_wiki(
    db: &Connection,
    out_dir: &Path,
    max_key_nodes: usize,
    root: Option<&Path>,
) -> Result<usize> {
    let wiki = Wiki::load(db, root)?;
    let hyperedges = astria_build::hyperedges::load_all(db).unwrap_or_default();
    let max_key_nodes = max_key_nodes.max(1);

    let communities_dir = out_dir.join("communities");
    let nodes_dir = out_dir.join("nodes");
    std::fs::create_dir_all(&communities_dir)?;
    std::fs::create_dir_all(&nodes_dir)?;

    // Community id -> member node ids.
    let mut members: HashMap<i64, Vec<&String>> = HashMap::new();
    for (id, node) in &wiki.nodes {
        if let Some(cid) = node.community {
            members.entry(cid).or_default().push(id);
        }
    }

    let mut count = 0usize;
    for (cid, label, cohesion) in &wiki.communities {
        let member_ids = members.remove(cid).unwrap_or_default();
        let stem = wiki
            .community_stems
            .get(label)
            .cloned()
            .unwrap_or_else(|| slug(label));
        let article = community_article(&wiki, label, *cohesion, &member_ids, max_key_nodes);
        std::fs::write(communities_dir.join(format!("{stem}.md")), article)?;
        count += 1;
    }

    for node in &wiki.god_nodes {
        let stem = match wiki.node_stems.get(&node.label) {
            Some(stem) => stem.clone(),
            None => continue,
        };
        let article = god_node_article(&wiki, node);
        std::fs::write(nodes_dir.join(format!("{stem}.md")), article)?;
        count += 1;
    }

    let member_counts: HashMap<i64, usize> =
        wiki.nodes
            .values()
            .filter_map(|n| n.community)
            .fold(HashMap::new(), |mut acc, cid| {
                *acc.entry(cid).or_insert(0) += 1;
                acc
            });
    std::fs::write(
        out_dir.join("index.md"),
        index_md(&wiki, &member_counts, wiki.edges.len(), &hyperedges),
    )?;

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    fn seed_basic_db() -> Connection {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file, community, signature) VALUES
              ('a', 'Alpha()', 'code', 'src/a.rs', 1, 'pub fn alpha()'),
              ('b', 'Beta()', 'code', 'src/b.rs', 1, NULL),
              ('c', 'Gamma()', 'code', 'src/c.rs', 2, NULL),
              ('d', 'Delta()', 'code', 'src/d.rs', 2, NULL);
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('a', 'b', 'calls', 'EXTRACTED', 'src/a.rs'),
              ('b', 'a', 'uses', 'EXTRACTED', 'src/b.rs'),
              ('b', 'a', 'uses', 'INFERRED', 'src/b.rs'),
              ('a', 'c', 'imports', 'INFERRED', 'src/a.rs');
            INSERT INTO communities (id, label, cohesion, size) VALUES
              (1, 'Core', 0.75, 2),
              (2, 'Edge', 0.5, 1);
            ",
        )
        .unwrap();
        db
    }

    #[test]
    fn writes_index_community_and_god_node_articles() {
        let db = seed_basic_db();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("wiki");
        let count = export_wiki(&db, &out, 25, None).unwrap();

        // 2 community articles + god node articles (top-degree nodes)
        assert!(count >= 2);
        assert!(out.join("index.md").exists());
        assert!(out.join("communities").join("Core.md").exists());
        assert!(out.join("communities").join("Edge.md").exists());

        let index = std::fs::read_to_string(out.join("index.md")).unwrap();
        assert!(index.contains("[Core](communities/Core.md)"));
        assert!(index.contains("4 nodes · 4 edges · 2 communities"));
        assert!(index.contains("Surprising Connections"));
        // the lone cross-community edge (a -imports-> c) is the surprise
        assert!(index.contains("--imports-->"));

        let core = std::fs::read_to_string(out.join("communities").join("Core.md")).unwrap();
        assert!(core.contains("## Key Concepts"));
        assert!(core.contains("**Alpha()** (4 connections)"));
        // cross-community link to Edge + audit trail from touching edges
        assert!(core.contains("[Edge](../communities/Edge.md)"));
        assert!(core.contains("EXTRACTED: 2 (50%)"));
        assert!(core.contains("INFERRED: 2 (50%)"));
        assert!(core.contains("[index](../index.md)"));

        // god node article: Alpha() is top degree, gets nodes/<slug>.md
        let alpha_path = out.join("nodes").join("Alpha().md");
        assert!(alpha_path.exists());
        let alpha = std::fs::read_to_string(alpha_path).unwrap();
        assert!(alpha.contains("God node · 4 connections"));
        assert!(alpha.contains("**Community:** [Core](../communities/Core.md)"));
        assert!(alpha.contains("## Signature"));
        assert!(alpha.contains("pub fn alpha()"));
        assert!(alpha.contains("### calls"));
        // duplicate uses edges (EXTRACTED + INFERRED) collapse to one entry
        let uses_block = alpha.split("### uses").nth(1).unwrap();
        assert_eq!(uses_block.matches("- [Beta()]").count(), 1);
        assert!(uses_block.contains("`EXTRACTED`"));
        assert!(!uses_block.contains("`INFERRED`"));
    }

    #[test]
    fn root_prefix_strips_to_relative_paths() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file, community) VALUES
              ('a', 'Alpha()', 'code', 'C:/repo/src/a.rs', 1);
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('a', 'a', 'uses', 'EXTRACTED', 'C:/repo/src/a.rs');
            INSERT INTO communities (id, label, size) VALUES (1, 'Core', 1);
            ",
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("wiki");
        export_wiki(&db, &out, 25, Some(Path::new("C:/repo"))).unwrap();

        let core = std::fs::read_to_string(out.join("communities").join("Core.md")).unwrap();
        assert!(core.contains("`src/a.rs`"));
        assert!(!core.contains("C:/repo"));
    }

    #[test]
    fn key_concepts_truncate_with_more_line() {
        let db = open_db_in_memory().unwrap();
        for i in 0..6 {
            db.execute(
                "INSERT INTO nodes (id, label, file_type, source_file, community) VALUES (?1, ?2, 'code', 'x/f.rs', 1)",
                rusqlite::params![format!("n{i}"), format!("N{i}()")],
            )
            .unwrap();
        }
        db.execute_batch("INSERT INTO communities (id, label, size) VALUES (1, 'Big', 6);")
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("wiki");
        export_wiki(&db, &out, 3, None).unwrap();

        let article = std::fs::read_to_string(out.join("communities").join("Big.md")).unwrap();
        assert!(article.contains("*... and 3 more nodes in this community*"));
    }

    #[test]
    fn colliding_labels_get_unique_stems() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file, community) VALUES
              ('a', 'A', 'code', 'f.rs', 1),
              ('b', 'B', 'code', 'g.rs', 2);
            INSERT INTO communities (id, label, size) VALUES
              (1, 'Mod/One', 1),
              (2, 'Mod:One', 1);
            ",
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("wiki");
        export_wiki(&db, &out, 25, None).unwrap();

        // Both labels slug to Mod-One; the second becomes Mod-One-2
        assert!(out.join("communities").join("Mod-One.md").exists());
        assert!(out.join("communities").join("Mod-One-2.md").exists());
    }

    #[test]
    fn slug_sanitizes_hostile_characters() {
        assert_eq!(slug("a/b\\c:d"), "a-b-c-d");
        assert_eq!(slug("hello world"), "hello_world");
        assert_eq!(slug("..."), "unnamed");
        assert_eq!(slug("-astria"), "astria");
        assert_eq!(slug("NUL"), "_NUL");
        assert_eq!(slug("COM1"), "_COM1");
        let long: String = "x".repeat(200);
        assert!(slug(&long).len() <= MAX_SLUG_LEN);
    }
}
