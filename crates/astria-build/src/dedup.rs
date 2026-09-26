// Entity dedup: merge near-duplicate nodes (same symbol spelled differently)
// after build, before clustering.
// Ported from upstream astria v8 dedup.py: normalize → entropy gate →
// MinHash/LSH blocking → Jaro-Winkler verify with same-community boost →
// union-find merge with anti-overmerge guards.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use unicode_normalization::UnicodeNormalization;

use crate::minhash::{band_key, char_trigram_shingles, signature, BANDS};

use astria_core::Result;

/// Minimum Shannon entropy (bits/char) for a label to participate —
/// low-information labels ("ab", "xyz", "test") merge too aggressively.
const MIN_ENTROPY: f64 = 2.5;
/// Jaro-Winkler similarity threshold (percent) with community boost applied.
const JW_THRESHOLD: f64 = 92.0;
/// Boost (in percent points) when both nodes are in the same community.
const SAME_COMMUNITY_BOOST: f64 = 5.0;
/// Labels at or below this length only merge on near-identity.
const SHORT_LABEL_LEN: usize = 4;

struct NodeRow {
    id: String,
    label: String,
    norm: String,
    file_type: String,
    source_file: String,
    community: Option<i64>,
}

/// NFKC + casefold + non-alphanumeric → space, collapsed.
fn normalize_label(label: &str) -> String {
    let mut normalized: String = label.chars().flat_map(|c| c.to_lowercase()).collect();
    normalized = normalized.nfkc().collect();
    let mut out = String::with_capacity(normalized.len());
    let mut pending_space = false;
    for ch in normalized.chars() {
        if ch.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(ch);
        } else if !out.is_empty() {
            pending_space = true;
        }
    }
    out
}

/// Shannon entropy of the normalized label, bits per character.
fn entropy(s: &str) -> f64 {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return 0.0;
    }
    let mut counts: HashMap<char, usize> = HashMap::new();
    for &c in &chars {
        *counts.entry(c).or_insert(0) += 1;
    }
    let n = chars.len() as f64;
    -counts
        .values()
        .map(|&k| {
            let p = k as f64 / n;
            p * p.log2()
        })
        .sum::<f64>()
}

/// Multiset of digit runs ("v2 beta" → {2}, "user service v3" → {3}) —
/// labels whose digit runs differ describe different versions, never merge.
fn digit_runs(s: &str) -> Vec<&str> {
    let mut runs = Vec::new();
    for token in s.split(|c: char| !c.is_ascii_digit()) {
        if !token.is_empty() {
            runs.push(token);
        }
    }
    runs.sort_unstable();
    runs
}

/// Same set of non-digit tokens in a different order ("error handler" vs
/// "handler error") is usually two different concepts, not a duplicate.
fn sorted_tokens(s: &str) -> Vec<&str> {
    let mut t: Vec<&str> = s
        .split(' ')
        .filter(|tok| !tok.is_empty() && tok.chars().any(|c| !c.is_ascii_digit()))
        .collect();
    t.sort_unstable();
    t
}

fn token_swap_blocked(a: &str, b: &str) -> bool {
    sorted_tokens(a) == sorted_tokens(b) && a != b
}

/// A strict token subset/superset means one name has an extra qualifier
/// ("normalize id" vs "normalize") — a different symbol, not a duplicate.
/// Jaro-Winkler's prefix weighting would otherwise merge qualified names
/// into their own prefixes.
fn token_subset_blocked(a: &str, b: &str) -> bool {
    let (ta, tb) = (sorted_tokens(a), sorted_tokens(b));
    let (small, large) = if ta.len() <= tb.len() {
        (&ta, &tb)
    } else {
        (&tb, &ta)
    };
    !small.is_empty() && small.len() < large.len() && {
        let large_set: std::collections::HashSet<&&str> = large.iter().collect();
        small.iter().all(|t| large_set.contains(t))
    }
}

/// Short labels only merge on near-identity (same length, ≤1 substitution).
fn short_label_blocked(a: &str, b: &str) -> bool {
    let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if ac.len() > SHORT_LABEL_LEN && bc.len() > SHORT_LABEL_LEN {
        return false;
    }
    if ac.len() != bc.len() {
        return true;
    }
    ac.iter().zip(bc.iter()).filter(|(x, y)| x != y).count() > 1
}

/// Non-code nodes from different files stay separate — document concepts are
/// merged by id at build time; fuzzy-merging across files loses provenance.
fn cross_file_noncode_blocked(a: &NodeRow, b: &NodeRow) -> bool {
    let noncode = |ft: &str| matches!(ft, "document" | "paper" | "image" | "video");
    noncode(&a.file_type) && noncode(&b.file_type) && a.source_file != b.source_file
}

/// What kind of entity the raw label names. Symbols end with "()"
/// ("validate_url()"), files carry an extension ("validate.rs", "sample.go");
/// everything else (prose headings, concept names) is free-form.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Symbol,
    File,
    Free,
}

fn label_shape(label: &str) -> Shape {
    if label.ends_with("()") {
        return Shape::Symbol;
    }
    if let Some(dot) = label.rfind('.') {
        let ext = &label[dot + 1..];
        if dot > 0
            && !ext.is_empty()
            && ext.len() <= 5
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Shape::File;
        }
    }
    Shape::Free
}

/// A symbol, a file, and a free-form concept are different kinds of entity
/// even when their names look alike: JW("validate url", "validate rs") ≈ 94.9
/// would otherwise merge `validate_url()` into the `validate.rs` file node,
/// deleting the symbol and rewiring its callers to the file.
fn shape_mismatch_blocked(a: &NodeRow, b: &NodeRow) -> bool {
    !matches!(
        (label_shape(&a.label), label_shape(&b.label)),
        (Shape::Symbol, Shape::Symbol) | (Shape::File, Shape::File) | (Shape::Free, Shape::Free)
    )
}

/// Two files that share a name are still two files: `install/index.ts` and
/// `src/index.ts` are different entities, and merging them erases one
/// file's identity (its id comes to point at the other path). File-shaped
/// nodes therefore only merge when they come from the same source file.
fn same_file_guard(a: &NodeRow, b: &NodeRow) -> bool {
    let is_file = |l: &str| label_shape(l) == Shape::File;
    is_file(&a.label) && is_file(&b.label) && a.source_file != b.source_file
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}

/// Merge near-duplicate nodes. Returns the number of nodes removed.
pub fn dedup_nodes(db: &Connection) -> Result<usize> {
    // Load candidate nodes (skip stubs and already-merged empty labels)
    let mut rows: Vec<NodeRow> = Vec::new();
    {
        let mut stmt =
            db.prepare("SELECT id, label, file_type, source_file, community FROM nodes")?;
        let loaded = stmt.query_map([], |row| {
            Ok(NodeRow {
                id: row.get(0)?,
                label: row.get(1)?,
                norm: String::new(),
                file_type: row.get(2)?,
                source_file: row.get(3)?,
                community: row.get(4)?,
            })
        })?;
        for mut r in loaded.flatten() {
            // Stubs are speculative; files are identities. Neither is a
            // fuzzy-dedup candidate - and a file node must never merge away,
            // because transitive union chains can defeat pair-level guards
            // (index.ts ~ index.tsx ~ install/index.ts erases an entry file).
            if r.file_type == "stub" || r.label.is_empty() || label_shape(&r.label) == Shape::File {
                continue;
            }
            r.norm = normalize_label(&r.label);
            if r.norm.is_empty() || entropy(&r.norm) < MIN_ENTROPY {
                continue;
            }
            rows.push(r);
        }
    }

    let n = rows.len();
    if n < 2 {
        return Ok(0);
    }

    // MinHash signatures + LSH banding for candidate pairs
    let sigs: Vec<Vec<u64>> = rows
        .iter()
        .map(|r| signature(&char_trigram_shingles(&r.norm)))
        .collect();
    let mut buckets: HashMap<(usize, u64), Vec<usize>> = HashMap::new();
    for (i, sig) in sigs.iter().enumerate() {
        for band in 0..BANDS {
            buckets
                .entry((band, band_key(sig, band)))
                .or_default()
                .push(i);
        }
    }
    let mut candidate_pairs: HashSet<(usize, usize)> = HashSet::new();
    for group in buckets.values() {
        if group.len() < 2 {
            continue;
        }
        for i in 0..group.len() {
            for j in (i + 1)..group.len() {
                let (a, b) = (group[i], group[j]);
                candidate_pairs.insert((a.min(b), a.max(b)));
            }
        }
    }

    // Verify candidates and union accepted merges
    let mut uf = UnionFind::new(n);
    let mut merges = 0usize;
    for &(i, j) in &candidate_pairs {
        let (a, b) = (&rows[i], &rows[j]);
        let mut score = strsim::jaro_winkler(&a.norm, &b.norm) * 100.0;
        if let (Some(ca), Some(cb)) = (a.community, b.community) {
            if ca == cb {
                score += SAME_COMMUNITY_BOOST;
            }
        }
        if score < JW_THRESHOLD {
            continue;
        }
        // Anti-overmerge guards
        if digit_runs(&a.norm) != digit_runs(&b.norm) {
            continue;
        }
        if token_swap_blocked(&a.norm, &b.norm) {
            continue;
        }
        if token_subset_blocked(&a.norm, &b.norm) {
            continue;
        }
        if short_label_blocked(&a.norm, &b.norm) {
            continue;
        }
        if cross_file_noncode_blocked(a, b) {
            continue;
        }
        if shape_mismatch_blocked(a, b) {
            continue;
        }
        if same_file_guard(a, b) {
            continue;
        }
        if uf.find(i) != uf.find(j) {
            uf.union(i, j);
            merges += 1;
        }
    }
    if merges == 0 {
        return Ok(0);
    }

    // Pick a survivor per group: shortest label (most canonical), tie → id
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = uf.find(i);
        groups.entry(root).or_default().push(i);
    }

    let mut rename: HashMap<String, String> = HashMap::new();
    for members in groups.values() {
        if members.len() < 2 {
            continue;
        }
        let mut members = members.clone();
        members.sort_by(|&x, &y| {
            rows[x]
                .label
                .len()
                .cmp(&rows[y].label.len())
                .then_with(|| rows[x].id.cmp(&rows[y].id))
        });
        let survivor = members[0];
        for &loser in &members[1..] {
            rename.insert(rows[loser].id.clone(), rows[survivor].id.clone());
        }
    }

    apply_renames(db, &rename)?;
    Ok(rename.len())
}

/// Rewrite edges to survivors, drop self-loops, delete merged nodes.
fn apply_renames(db: &Connection, rename: &HashMap<String, String>) -> Result<()> {
    let tx = db.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare("SELECT id, source, target FROM edges")?;
        let edges: Vec<(i64, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .flatten()
            .collect();
        drop(stmt);
        let mut update = tx.prepare("UPDATE edges SET source = ?1, target = ?2 WHERE id = ?3")?;
        let mut delete = tx.prepare("DELETE FROM edges WHERE id = ?1")?;
        for (id, source, target) in edges {
            let new_source = rename.get(&source).unwrap_or(&source).clone();
            let new_target = rename.get(&target).unwrap_or(&target).clone();
            if new_source == new_target {
                delete.execute(rusqlite::params![id])?;
            } else if new_source != source || new_target != target {
                update.execute(rusqlite::params![new_source, new_target, id])?;
            }
        }
    }
    let losers: Vec<&String> = rename.keys().collect();
    {
        let mut delete_node = tx.prepare("DELETE FROM nodes WHERE id = ?1")?;
        for loser in losers {
            delete_node.execute(rusqlite::params![loser])?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    fn insert(
        db: &Connection,
        id: &str,
        label: &str,
        ft: &str,
        file: &str,
        community: Option<i64>,
    ) {
        db.execute(
            "INSERT INTO nodes (id, label, file_type, source_file, community) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, label, ft, file, community],
        )
        .unwrap();
    }

    fn edge(db: &Connection, source: &str, target: &str) {
        db.execute(
            "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES (?1, ?2, 'calls', 'EXTRACTED', 'f.rs')",
            rusqlite::params![source, target],
        )
        .unwrap();
    }

    #[test]
    fn merges_casing_and_separator_variants() {
        let db = open_db_in_memory().unwrap();
        insert(&db, "a", "UserService", "code", "a.rs", Some(1));
        insert(&db, "b", "user service", "code", "b.rs", Some(1));
        insert(&db, "c", "userservice", "code", "c.rs", Some(1));
        insert(&db, "caller1", "caller()", "code", "d.rs", Some(2));
        edge(&db, "caller1", "a");
        edge(&db, "a", "b"); // becomes a self-loop after merge → dropped
        let removed = dedup_nodes(&db).unwrap();
        assert_eq!(removed, 2);
        let survivors: Vec<String> = db
            .prepare("SELECT id FROM nodes WHERE id IN ('a','b','c')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(survivors, vec!["a".to_string()]);
        // self-loop dropped, external edge rewired
        let (loops, rewired): (i64, i64) = db
            .query_row(
                "SELECT SUM(CASE WHEN source = target THEN 1 ELSE 0 END),
                        SUM(CASE WHEN source = 'caller1' AND target = 'a' THEN 1 ELSE 0 END)
                 FROM edges",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(loops, 0);
        assert_eq!(rewired, 1);
    }

    #[test]
    fn digit_run_versions_not_merged() {
        let db = open_db_in_memory().unwrap();
        insert(&db, "a", "ConnectionPoolV2", "code", "a.rs", Some(1));
        insert(&db, "b", "ConnectionPoolV3", "code", "b.rs", Some(1));
        let removed = dedup_nodes(&db).unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn token_swap_not_merged() {
        let db = open_db_in_memory().unwrap();
        insert(&db, "a", "Error Handler Service", "code", "a.rs", Some(1));
        insert(&db, "b", "Handler Error Service", "code", "b.rs", Some(1));
        let removed = dedup_nodes(&db).unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn qualified_names_not_merged_into_prefix() {
        let db = open_db_in_memory().unwrap();
        insert(&db, "a", "normalize_id", "code", "a.rs", Some(1));
        insert(&db, "b", "normalize", "code", "b.rs", Some(1));
        assert_eq!(dedup_nodes(&db).unwrap(), 0);
    }

    #[test]
    fn short_labels_need_near_identity() {
        let db = open_db_in_memory().unwrap();
        insert(&db, "a", "ab", "code", "a.rs", Some(1));
        insert(&db, "b", "abc", "code", "b.rs", Some(1));
        // different lengths → blocked
        assert_eq!(dedup_nodes(&db).unwrap(), 0);
    }

    #[test]
    fn low_entropy_labels_skipped() {
        let db = open_db_in_memory().unwrap();
        insert(&db, "a", "aaaa", "code", "a.rs", Some(1));
        insert(&db, "b", "aaaaa", "code", "b.rs", Some(1));
        // entropy of "aaaa" ≈ 0 → never candidates
        assert_eq!(dedup_nodes(&db).unwrap(), 0);
    }

    #[test]
    fn cross_file_documents_not_merged() {
        let db = open_db_in_memory().unwrap();
        insert(
            &db,
            "a",
            "Attention Mechanism",
            "document",
            "paper1.md",
            None,
        );
        insert(
            &db,
            "b",
            "Attention Mechanisms",
            "document",
            "paper2.md",
            None,
        );
        assert_eq!(dedup_nodes(&db).unwrap(), 0);
    }

    #[test]
    fn same_name_files_in_different_dirs_stay_separate() {
        // Reproduces the q17 incident: `src/index.ts` (the CLI entry) and
        // `install/index.ts` shared the label "index.ts", and the merge
        // erased the entry file's node. Same-named files in different
        // directories are different files.
        let db = open_db_in_memory().unwrap();
        insert(
            &db,
            "install_index",
            "index.ts",
            "code",
            "packages/astria-cli/src/install/index.ts",
            Some(1),
        );
        insert(
            &db,
            "src_index",
            "index.ts",
            "code",
            "packages/astria-cli/src/index.ts",
            Some(1),
        );
        assert_eq!(dedup_nodes(&db).unwrap(), 0);
        let both: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE id IN ('install_index','src_index')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(both, 2);
    }

    #[test]
    fn file_nodes_do_not_absorb_symbols() {
        // JW("validate url", "validate rs") ≈ 94.9 clears the 92 threshold —
        // without the shape guard the `validate.rs` file node deleted the
        // `validate_url()` symbol and its callers pointed at the file.
        let db = open_db_in_memory().unwrap();
        insert(
            &db,
            "src_validate",
            "validate.rs",
            "code",
            "validate.rs",
            Some(1),
        );
        insert(
            &db,
            "src_lib::validate_url",
            "validate_url()",
            "code",
            "ingest/lib.rs",
            Some(1),
        );
        assert_eq!(dedup_nodes(&db).unwrap(), 0);
        let kept: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE id = 'src_lib::validate_url'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, 1);
    }

    #[test]
    fn no_candidates_is_noop() {
        let db = open_db_in_memory().unwrap();
        insert(
            &db,
            "a",
            "database_connection_pool",
            "code",
            "a.rs",
            Some(1),
        );
        insert(&db, "b", "html_render_visitor", "code", "b.rs", Some(2));
        assert_eq!(dedup_nodes(&db).unwrap(), 0);
    }
}
