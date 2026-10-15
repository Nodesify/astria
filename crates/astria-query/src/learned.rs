//! Learned-edge bookkeeping: query feedback pairs and their promotion
//! into `learned` edges once they recur across distinct questions.
use super::*;

/// Canonical question key shared with the curated memory loop: lowercase,
/// whitespace-collapsed, bounded. `query_pairs` and `memory_outcomes` must
/// agree on this shape or outcome verdicts cannot steer promotion.
pub fn normalize_question(question: &str) -> String {
    question
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

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
    let question = normalize_question(question);
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

    // Outcome gate: a pair backed ONLY by questions that were later
    // judged dead ends or corrections is repeated unsuccessful
    // exploration, not corroborated knowledge — promotion would turn it
    // into an authoritative learned edge. At least one surviving
    // (non-failed) distinct question must vouch for the pair.
    let pairs: Vec<(String, String, i64)> = {
        let mut stmt = db.prepare(
            "SELECT qp.source, qp.target, SUM(qp.hits) FROM query_pairs qp
             LEFT JOIN memory_outcomes mo ON mo.question = qp.question
             GROUP BY qp.source, qp.target
             HAVING COUNT(DISTINCT qp.question) >= ?1 AND SUM(qp.hits) >= ?2
               AND COUNT(DISTINCT CASE WHEN mo.outcome IN ('dead_end','corrected')
                                        THEN qp.question END)
                   < COUNT(DISTINCT qp.question)",
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

#[cfg(test)]
mod outcome_tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    fn seed_pair(db: &Connection, source: &str, target: &str, question: &str, hits: i64) {
        db.execute(
            "INSERT OR REPLACE INTO nodes (id, label, file_type, source_file) VALUES (?1, ?1, 'code', 'f.rs')",
            rusqlite::params![source],
        )
        .unwrap();
        db.execute(
            "INSERT OR REPLACE INTO nodes (id, label, file_type, source_file) VALUES (?1, ?1, 'code', 'f.rs')",
            rusqlite::params![target],
        )
        .unwrap();
        db.execute(
            "INSERT INTO query_pairs (source, target, question, hits, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, 1, 1)",
            rusqlite::params![source, target, question, hits],
        )
        .unwrap();
    }

    #[test]
    fn all_failed_questions_do_not_promote() {
        let db = open_db_in_memory().unwrap();
        // Two distinct questions both later judged dead ends: repeated
        // unsuccessful exploration must not become an authoritative edge.
        seed_pair(&db, "a", "b", "how does x work", 3);
        seed_pair(&db, "a", "b", "where is x used", 2);
        db.execute(
            "INSERT INTO memory_outcomes (question, outcome, updated_at) VALUES
             ('how does x work', 'dead_end', 1), ('where is x used', 'dead_end', 1)",
            [],
        )
        .unwrap();
        let promoted = promote_learned_edges(&db, 2, 2).unwrap();
        assert_eq!(promoted, 0, "all-dead-end pairs must not promote");
        // The rows survive (the exploration history is real); only
        // promotion is gated.
        let pairs: i64 = db
            .query_row("SELECT COUNT(*) FROM query_pairs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(pairs, 2);
    }

    #[test]
    fn one_surviving_question_still_promotes() {
        let db = open_db_in_memory().unwrap();
        seed_pair(&db, "a", "b", "how does x work", 3);
        seed_pair(&db, "a", "b", "where is x used", 2);
        // Only one of the two questions failed; corroboration survives.
        db.execute(
            "INSERT INTO memory_outcomes (question, outcome, updated_at)
             VALUES ('how does x work', 'dead_end', 1)",
            [],
        )
        .unwrap();
        let promoted = promote_learned_edges(&db, 2, 2).unwrap();
        assert_eq!(promoted, 1);
    }
}
