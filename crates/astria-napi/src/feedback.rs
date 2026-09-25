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
    std::fs::write(&memory_path, &body)?;

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

/// Aggregate memory docs into `astria-out/reflections/LESSONS.md`.
pub fn reflect(astria_dir: &Path) -> Result<String> {
    let memory_dir = astria_dir.join("memory");
    let mut total = 0usize;
    let mut outcomes: std::collections::HashMap<String, usize> = Default::default();
    let mut entries: Vec<(String, String)> = Vec::new(); // (question, outcome)

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
            total += 1;
            let mut question = String::new();
            let mut outcome = "unrecorded".to_string();
            for line in text.lines() {
                if let Some(q) = line.strip_prefix("question: ") {
                    question = q.to_string();
                } else if let Some(o) = line.strip_prefix("outcome: ") {
                    outcome = o.trim().to_string();
                }
                if !line.starts_with("---") && line.starts_with("# Q: ") {
                    question = line[5..].trim_start_matches("Q:").trim().to_string();
                }
            }
            *outcomes.entry(outcome.clone()).or_insert(0) += 1;
            entries.push((question, outcome));
        }
    }

    let lessons_dir = astria_dir.join("reflections");
    std::fs::create_dir_all(&lessons_dir)?;
    let mut out = String::from("# Lessons\n\n");
    out.push_str(&format!("Memory entries: {total}\n\n"));
    let mut tallies: Vec<(String, usize)> = outcomes.into_iter().collect();
    tallies.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (kind, count) in &tallies {
        out.push_str(&format!("- **{kind}**: {count}\n"));
    }
    if !entries.is_empty() {
        out.push_str("\n## Recorded Q/A pairs\n\n");
        for (question, outcome) in &entries {
            out.push_str(&format!("- [{outcome}] {question}\n"));
        }
    }
    let lessons_path = lessons_dir.join("LESSONS.md");
    std::fs::write(&lessons_path, &out)?;
    Ok(out)
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
        let lessons = reflect(&gdir).unwrap();
        assert!(lessons.contains("Memory entries: 2"));
        assert!(lessons.contains("**useful**: 1"));
        assert!(lessons.contains("**dead_end**: 1"));
        assert!(gdir.join("reflections").join("LESSONS.md").exists());
    }

    #[test]
    fn invalid_outcome_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db_in_memory().unwrap();
        assert!(save_result(&db, dir.path(), "q", "a", Some("meh"), None, &[]).is_err());
    }
}
