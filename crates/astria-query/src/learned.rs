//! Learned-edge bookkeeping: query feedback pairs and their promotion
//! into `learned` edges once they recur across distinct questions.
use super::*;

/// Feedback loop bookkeeping: for one answered query, record which
/// (seed, discovered) node pairs the traversal connected. Pairs recurring
/// across DISTINCT questions later promote into `learned` edges — the
/// graph remembers which connections users actually keep asking about.
pub(crate) fn record_query_pairs(
    db: &Connection,
    loaded: &LoadedGraph,
    seeds: &[NodeIndex],
    visited: &HashSet<NodeIndex>,
    question: &str,
) {
    let question: String = question
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect();
    if question.is_empty() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    // Top seeds by rank; discoveries ranked by degree — the load-bearing
    // nodes the question actually reached.
    let top_seeds: Vec<&NodeIndex> = seeds.iter().take(3).collect();
    let mut discoveries: Vec<(usize, &NodeIndex)> = visited
        .iter()
        .filter(|idx| !seeds.contains(idx))
        .map(|idx| (loaded.graph.neighbors(*idx).count(), idx))
        .collect();
    discoveries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    let top_discoveries: Vec<&NodeIndex> =
        discoveries.iter().map(|(_, idx)| *idx).take(5).collect();

    for seed in &top_seeds {
        for discovery in &top_discoveries {
            let (source, target) = if loaded.graph[**seed].id <= loaded.graph[**discovery].id {
                (&loaded.graph[**seed].id, &loaded.graph[**discovery].id)
            } else {
                (&loaded.graph[**discovery].id, &loaded.graph[**seed].id)
            };
            let _ = db.execute(
                "INSERT INTO query_pairs (source, target, question, hits, first_seen, last_seen)
                 VALUES (?1, ?2, ?3, 1, ?4, ?4)
                 ON CONFLICT (source, target, question)
                 DO UPDATE SET hits = hits + 1, last_seen = ?4",
                rusqlite::params![source, target, question, ts],
            );
        }
    }
}

/// Promote recurring query pairs into `learned` edges: a pair qualifies
/// when it was connected by at least `min_questions` DISTINCT questions
/// with at least `min_hits` total repetitions. Existing learned edges are
/// regenerated (idempotent). Learned edges carry confidence INFERRED with
/// a hits-based score, so `--detail high` traversals can filter them.
/// Returns the number of learned edges materialized.
pub fn promote_learned_edges(
    db: &Connection,
    min_questions: usize,
    min_hits: usize,
) -> astria_core::Result<usize> {
    // Drop pairs whose endpoints were deleted by later builds — nodes come
    // and go with files, and a stale reference would violate the edges FK.
    db.execute(
        "DELETE FROM query_pairs WHERE source NOT IN (SELECT id FROM nodes)
         OR target NOT IN (SELECT id FROM nodes)",
        [],
    )?;

    let pairs: Vec<(String, String, i64)> = {
        let mut stmt = db.prepare(
            "SELECT source, target, SUM(hits) FROM query_pairs
             GROUP BY source, target
             HAVING COUNT(DISTINCT question) >= ?1 AND SUM(hits) >= ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![min_questions as i64, min_hits as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        rows.flatten().collect()
    };

    // Promotion is the only writer of `learned` edges and regenerates them
    // from query_pairs, so drop every learned edge regardless of the
    // source_file stamp — a stale row from an older convention would
    // otherwise survive next to its regenerated twin.
    db.execute("DELETE FROM edges WHERE relation = 'learned'", [])?;

    let tx = db.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file)
             VALUES (?1, ?2, 'learned', 'INFERRED', ?3, 'query_history')",
        )?;
        for (source, target, hits) in &pairs {
            // 3 hits -> 0.5, 10+ hits -> 1.0: recency-weighted importance
            // without letting a single hot pair dominate high-fidelity views.
            let score = ((*hits as f64 - 1.0) / 9.0).clamp(0.1, 1.0);
            stmt.execute(rusqlite::params![source, target, score])?;
        }
    }
    tx.commit()?;
    Ok(pairs.len())
}
