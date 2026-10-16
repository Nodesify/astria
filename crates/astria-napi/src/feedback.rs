// feedback: the curated memory loop (port of upstream save-result/reflect).
// save-result writes a memory markdown doc AND inserts a document node wired
// to the source nodes it cites; reflect aggregates outcomes across memory
// docs into LESSONS.md for agents to read. Complements the automatic
// learned-edge promotion (query_pairs) with human/agent-curated verdicts.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use astria_core::AstriaError;
use astria_core::Result;

pub struct SavedResult {
    pub memory_path: PathBuf,
    pub node_id: String,
}

/// Outcome vocabulary — must match the frontmatter the extractor may read.
pub const OUTCOMES: &[&str] = &["useful", "dead_end", "corrected"];

/// Save a Q/A pair: memory doc on disk + document node + `references` edges
/// to cited nodes, all in one call.
pub fn save_result(
    db: &Connection,
    astria_dir: &Path,
    question: &str,
    answer: &str,
    outcome: Option<&str>,
    correction: Option<&str>,
    source_nodes: &[String],
) -> Result<SavedResult> {
    if question.trim().is_empty() || answer.trim().is_empty() {
        return Err(AstriaError::Graph(
            "question and answer must not be empty".into(),
        ));
    }
    if let Some(o) = outcome {
        if !OUTCOMES.contains(&o) {
            return Err(AstriaError::Graph(format!(
                "unknown outcome '{o}' — expected one of: {}",
                OUTCOMES.join(", ")
            )));
        }
    }

    // Record the verdict where learned-edge promotion can see it: the
    // question normalized exactly as query_pairs stores it, so repeated
    // unsuccessful exploration cannot promote into authoritative edges.
    // The latest verdict for a question wins (corrections evolve).
    if let Some(o) = outcome {
        let _ = db.execute(
            "INSERT INTO memory_outcomes (question, outcome, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(question) DO UPDATE SET outcome = ?2, updated_at = ?3",
            rusqlite::params![
                astria_query::normalize_question(question),
                o,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0) as i64,
            ],
        );
    }

    let memory_dir = astria_dir.join("memory");
    std::fs::create_dir_all(&memory_dir)?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let slug: String = {
        let raw: String = question
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        let collapsed: Vec<&str> = raw.split('-').filter(|s| !s.is_empty()).collect();
        collapsed.join("-").to_lowercase()
    };
    let slug = if slug.len() > 40 {
        slug[..40].to_string()
    } else {
        slug
    };
    let file_name = format!("query_{ts}_{slug}.md");
    let memory_path = memory_dir.join(&file_name);

    let mut frontmatter = String::from("---\ntype: memory\n");
    frontmatter.push_str(&format!("date: {ts}\n"));
    frontmatter.push_str(&format!("question: {}\n", question.replace('"', "'")));
    if let Some(o) = outcome {
        frontmatter.push_str(&format!("outcome: {o}\n"));
    }
    if let Some(c) = correction {
        frontmatter.push_str(&format!("correction: {}\n", c.replace('"', "'")));
    }
    if !source_nodes.is_empty() {
        frontmatter.push_str("source_nodes:\n");
        for n in source_nodes.iter().take(10) {
            frontmatter.push_str(&format!("  - {n}\n"));
        }
    }
    frontmatter.push_str("---\n");

    let mut body = frontmatter;
    body.push_str(&format!("\n# Q: {question}\n\n## Answer\n\n{answer}\n"));
    if let Some(c) = correction {
        body.push_str(&format!("\n## Correction\n\n{c}\n"));
    }
    body.push_str("\n## Outcome\n\n");
    body.push_str(outcome.unwrap_or("unrecorded"));
    body.push('\n');
    astria_core::writer_lock::write_atomic(&memory_path, body.as_bytes())?;

    // Graph side: one document node + references edges to cited nodes.
    let node_id = format!("memory_{ts}_{:x}", fnv64(&file_name));
    db.execute(
        "INSERT OR REPLACE INTO nodes (id, label, file_type, source_file, source_line, docstring)
         VALUES (?1, ?2, 'document', ?3, 1, ?4)",
        rusqlite::params![
            node_id,
            truncate(question, 80),
            astria_paths::normalize(&memory_path),
            answer.chars().take(200).collect::<String>(),
        ],
    )?;
    for target in source_nodes.iter().take(10) {
        // The cited node must exist; skip silently otherwise (stubs would
        // pollute the graph with speculative memory links).
        let exists: bool = db
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE id = ?1",
                rusqlite::params![target],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if !exists {
            continue;
        }
        db.execute(
            "INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file)
             VALUES (?1, ?2, 'references', 'EXTRACTED', 1.0, ?3)",
            rusqlite::params![
                node_id,
                target,
                astria_paths::normalize(&memory_path),
            ],
        )?;
    }

    Ok(SavedResult {
        memory_path,
        node_id,
    })
}

/// One parsed memory doc.
#[derive(Debug, Default, Clone)]
struct MemoryEntry {
    question: String,
    outcome: String,
    correction: Option<String>,
    date: u64,
    source_nodes: Vec<String>,
}

/// Aggregate memory docs into `.astria/reflections/LESSONS.md`. Beyond raw
/// outcome counts this is actionable: preferred sources (nodes corroborated
/// by multiple useful memories, most recent first), corrections to apply,
/// known dead ends, contested evidence (nodes both useful and dead-end
/// memories cite), and tentative findings (unrecorded outcomes) that need
/// verification. Citations of nodes deleted from the graph are discarded
/// when a connection is available. Lessons stay experiential — a separate
/// artifact from the structural graph facts, never merged into them.
pub fn reflect(astria_dir: &Path, db: Option<&Connection>) -> Result<String> {
    let memory_dir = astria_dir.join("memory");
    let mut entries: Vec<MemoryEntry> = Vec::new();

    if memory_dir.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&memory_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
            .collect();
        files.sort();
        for file in files {
            let text = match std::fs::read_to_string(&file) {
                Ok(t) => t,
                Err(_) => continue,
            };
            entries.push(parse_memory_doc(&text));
        }
    }

    // Drop citations of nodes the graph no longer has: a lesson pointing at
    // deleted code misleads, and the memory loop must self-clean.
    if let Some(db) = db {
        if let Ok(mut stmt) = db.prepare("SELECT id FROM nodes") {
            let live: std::collections::HashSet<String> = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default();
            for entry in &mut entries {
                entry.source_nodes.retain(|id| live.contains(id));
            }
        }
    }

    let mut outcomes: std::collections::HashMap<String, usize> = Default::default();
    for entry in &entries {
        *outcomes.entry(entry.outcome.clone()).or_insert(0) += 1;
    }

    // Preferred sources: useful-memory citations, corroborated by count and
    // recency (a node cited by several useful memories, most recently now,
    // is where future answers should start).
    let mut useful_citations: std::collections::HashMap<String, (usize, u64)> = Default::default();
    let mut dead_citations: std::collections::HashMap<String, usize> = Default::default();
    for entry in &entries {
        for node in &entry.source_nodes {
            if entry.outcome == "useful" {
                let slot = useful_citations.entry(node.clone()).or_insert((0, 0));
                slot.0 += 1;
                slot.1 = slot.1.max(entry.date);
            } else if entry.outcome == "dead_end" || entry.outcome == "corrected" {
                *dead_citations.entry(node.clone()).or_insert(0) += 1;
            }
        }
    }
    let node_label = |id: &str| -> Option<String> {
        db.and_then(|db| {
            db.query_row(
                "SELECT label FROM nodes WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get::<_, String>(0),
            )
            .ok()
        })
    };
    let mut preferred: Vec<(String, usize, u64)> = useful_citations
        .iter()
        .map(|(id, (count, last))| (id.clone(), *count, *last))
        .collect();
    preferred.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.cmp(&b.0))
    });

    let corrections: Vec<&MemoryEntry> = entries
        .iter()
        .filter(|e| {
            e.outcome == "corrected"
                && e.correction
                    .as_deref()
                    .is_some_and(|c| !c.trim().is_empty())
        })
        .collect();
    let dead_ends: Vec<&MemoryEntry> = entries.iter().filter(|e| e.outcome == "dead_end").collect();
    let tentative: Vec<&MemoryEntry> = entries
        .iter()
        .filter(|e| e.outcome == "unrecorded")
        .collect();

    let lessons_dir = astria_dir.join("reflections");
    std::fs::create_dir_all(&lessons_dir)?;
    let mut out = String::from("# Lessons\n\n");
    out.push_str(&format!(
        "Memory entries: {} — experiential lessons, kept separate from structural graph facts.\n\n",
        entries.len()
    ));
    let mut tallies: Vec<(String, usize)> = outcomes.into_iter().collect();
    tallies.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (kind, count) in &tallies {
        out.push_str(&format!("- **{kind}**: {count}\n"));
    }

    if !preferred.is_empty() {
        out.push_str("\n## Preferred sources (corroborated by useful memories)\n\n");
        for (id, count, last) in preferred.iter().take(10) {
            let label = node_label(id).unwrap_or_else(|| id.clone());
            out.push_str(&format!(
                "- {label} (`{id}`) — cited by {count} useful memories, last {last}\n"
            ));
        }
    }
    if !corrections.is_empty() {
        out.push_str("\n## Corrections to apply\n\n");
        for entry in corrections.iter().take(15) {
            let correction = entry.correction.as_deref().unwrap_or_default();
            out.push_str(&format!(
                "- {} — {correction}\n",
                truncate(&entry.question, 100)
            ));
        }
    }
    if !dead_ends.is_empty() {
        out.push_str("\n## Known dead ends (do not re-explore blindly)\n\n");
        for entry in dead_ends.iter().take(15) {
            out.push_str(&format!("- {}\n", truncate(&entry.question, 100)));
        }
    }
    // Contested evidence: cited as useful AND as dead-end/corrected.
    let contested: Vec<&String> = preferred
        .iter()
        .filter(|(id, _, _)| dead_citations.contains_key(id))
        .map(|(id, _, _)| id)
        .collect();
    if !contested.is_empty() {
        out.push_str("\n## Contested evidence (useful and dead-end memories disagree)\n\n");
        for id in contested.iter().take(10) {
            let label = node_label(id).unwrap_or_else(|| (*id).clone());
            out.push_str(&format!("- {label} (`{id}`)\n"));
        }
    }
    if !tentative.is_empty() {
        out.push_str("\n## Tentative findings (verify before relying on them)\n\n");
        for entry in tentative.iter().take(15) {
            out.push_str(&format!("- {}\n", truncate(&entry.question, 100)));
        }
    }

    let lessons_path = lessons_dir.join("LESSONS.md");
    astria_core::writer_lock::write_atomic(&lessons_path, out.as_bytes())?;
    Ok(out)
}

/// Frontmatter-aware parse of one memory doc: question, outcome,
/// correction, date, and the cited source node ids. Body `# Q:` lines are
/// the fallback for docs without frontmatter.
fn parse_memory_doc(text: &str) -> MemoryEntry {
    let mut entry = MemoryEntry {
        outcome: "unrecorded".to_string(),
        ..Default::default()
    };
    let mut in_frontmatter = false;
    let mut seen_first_fence = false;
    let mut in_sources = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            if !seen_first_fence {
                seen_first_fence = true;
                in_frontmatter = true;
                continue;
            } else if in_frontmatter {
                in_frontmatter = false;
                continue;
            }
        }
        if in_frontmatter {
            if let Some(rest) = trimmed.strip_prefix("source_nodes:") {
                in_sources = rest.trim().is_empty();
                continue;
            }
            if in_sources {
                if let Some(id) = trimmed.strip_prefix("- ") {
                    entry.source_nodes.push(id.trim().to_string());
                    continue;
                }
                in_sources = false;
            }
            if let Some(q) = trimmed.strip_prefix("question: ") {
                entry.question = q.trim_matches('"').to_string();
            } else if let Some(o) = trimmed.strip_prefix("outcome: ") {
                entry.outcome = o.trim().to_string();
            } else if let Some(c) = trimmed.strip_prefix("correction: ") {
                entry.correction = Some(c.trim_matches('"').to_string());
            } else if let Some(d) = trimmed.strip_prefix("date: ") {
                entry.date = d.trim().parse().unwrap_or(0);
            }
        } else if trimmed.starts_with("# Q: ") {
            entry.question = trimmed[5..].trim().to_string();
        }
    }
    entry
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

/// Tiny deterministic 64-bit FNV-1a for unique-ish memory node ids.
fn fnv64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    #[test]
    fn save_result_persists_doc_node_and_edges() {
        let dir = tempfile::tempdir().unwrap();
        let gdir = dir.path().join(".astria");
        std::fs::create_dir_all(&gdir).unwrap();
        let db = open_db_in_memory().unwrap();
        db.execute(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('tgt', 'target()', 'code', 'f.py')",
            [],
        )
        .unwrap();

        let saved = save_result(
            &db,
            &gdir,
            "How does auth work?",
            "Check the middleware",
            Some("useful"),
            None,
            &["tgt".to_string(), "missing_node".to_string()],
        )
        .unwrap();

        assert!(saved.memory_path.exists());
        let content = std::fs::read_to_string(&saved.memory_path).unwrap();
        assert!(content.contains("question: How does auth work?"));
        assert!(content.contains("outcome: useful"));
        // Node + edge to the existing target only.
        let node: (String, String) = db
            .query_row(
                "SELECT label, file_type FROM nodes WHERE id = ?1",
                rusqlite::params![saved.node_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(node.0, "How does auth work?");
        assert_eq!(node.1, "document");
        let edges: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE source = ?1 AND relation = 'references'",
                rusqlite::params![saved.node_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(edges, 1, "missing_node must not produce a speculative edge");
    }

    #[test]
    fn reflect_aggregates_outcomes() {
        let dir = tempfile::tempdir().unwrap();
        let gdir = dir.path().join(".astria");
        std::fs::create_dir_all(gdir.join("memory")).unwrap();
        let db = open_db_in_memory().unwrap();
        save_result(&db, &gdir, "Q one?", "A", Some("useful"), None, &[]).unwrap();
        save_result(&db, &gdir, "Q two?", "B", Some("dead_end"), None, &[]).unwrap();
        let lessons = reflect(&gdir, Some(&db)).unwrap();
        assert!(lessons.contains("Memory entries: 2"));
        assert!(lessons.contains("**useful**: 1"));
        assert!(lessons.contains("**dead_end**: 1"));
        assert!(gdir.join("reflections").join("LESSONS.md").exists());
    }

    #[test]
    fn reflect_is_actionable_and_drops_deleted_citations() {
        let dir = tempfile::tempdir().unwrap();
        let gdir = dir.path().join(".astria");
        std::fs::create_dir_all(gdir.join("memory")).unwrap();
        let db = open_db_in_memory().unwrap();
        db.execute(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('live', 'live()', 'code', 'f.py')",
            [],
        )
        .unwrap();
        // Useful memory citing a live node and a deleted node.
        save_result(
            &db,
            &gdir,
            "Where is auth?",
            "middleware",
            Some("useful"),
            None,
            &["live".to_string(), "deleted".to_string()],
        )
        .unwrap();
        save_result(
            &db,
            &gdir,
            "Where is auth exactly?",
            "middleware again",
            Some("useful"),
            None,
            &["live".to_string()],
        )
        .unwrap();
        // Dead end citing the same node: contested evidence.
        save_result(
            &db,
            &gdir,
            "Is auth in views?",
            "no",
            Some("dead_end"),
            None,
            &["live".to_string()],
        )
        .unwrap();
        // A correction to apply.
        save_result(
            &db,
            &gdir,
            "Old claim about cache?",
            "wrong",
            Some("corrected"),
            Some("cache lives in redis, not memory"),
            &[],
        )
        .unwrap();
        // Unverified memory.
        save_result(
            &db,
            &gdir,
            "Maybe about billing?",
            "unsure",
            None,
            None,
            &[],
        )
        .unwrap();

        let lessons = reflect(&gdir, Some(&db)).unwrap();
        assert!(lessons.contains("experiential lessons"), "{lessons}");
        assert!(lessons.contains("## Preferred sources"));
        assert!(
            lessons.contains("live()"),
            "preferred sources show labels: {lessons}"
        );
        assert!(lessons.contains("2 useful memories"));
        assert!(lessons.contains("## Known dead ends"));
        assert!(lessons.contains("Is auth in views?"));
        assert!(lessons.contains("## Corrections to apply"));
        assert!(lessons.contains("redis"));
        assert!(lessons.contains("## Contested evidence"));
        assert!(lessons.contains("## Tentative findings"));
        assert!(lessons.contains("Maybe about billing?"));
        // The deleted node's id must not appear anywhere.
        assert!(!lessons.contains("deleted"), "{lessons}");
    }

    #[test]
    fn save_result_records_outcome_for_promotion_gating() {
        let dir = tempfile::tempdir().unwrap();
        let gdir = dir.path().join(".astria");
        std::fs::create_dir_all(&gdir).unwrap();
        let db = open_db_in_memory().unwrap();
        save_result(
            &db,
            &gdir,
            "How does AUTH work?",
            "x",
            Some("dead_end"),
            None,
            &[],
        )
        .unwrap();
        let (outcome,): (String,) = db
            .query_row(
                "SELECT outcome FROM memory_outcomes WHERE question = 'how does auth work?'",
                [],
                |r| Ok((r.get(0)?,)),
            )
            .unwrap();
        assert_eq!(outcome, "dead_end");
    }

    #[test]
    fn invalid_outcome_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db_in_memory().unwrap();
        assert!(save_result(&db, dir.path(), "q", "a", Some("meh"), None, &[]).is_err());
    }
}
