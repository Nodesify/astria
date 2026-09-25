// astria-embed: local embedding model (fastembed/ONNX, no API key, no
// network after the first model download) powering semantic similarity
// edges and embedding-backed query recall.

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use rusqlite::Connection;
use std::path::PathBuf;

/// The embedding model used for nodes and queries. bge-small-en-v1.5
/// (384 dims) keeps the download small and embeddings fast.
pub const MODEL: EmbeddingModel = EmbeddingModel::BGESmallENV15;
/// Human-readable model label stored alongside embeddings.
pub const MODEL_NAME: &str = "BAAI/bge-small-en-v1.5";

/// Deterministic model cache shared across projects. Query-time model
/// loads are gated on `model_cached()` so they never trigger downloads.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = astria_core::env_var("EMBED_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".astria-embed-cache")
}

/// True when the model files are already downloaded — the only condition
/// under which query paths may load the embedder. fastembed 6 stores
/// models in HuggingFace-hub layout (`models--<org>--<name>/`), so probe
/// for any downloaded model directory.
pub fn model_cached() -> bool {
    let dir = cache_dir();
    match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with("models--")),
        Err(_) => false,
    }
}

/// Load the embedding model, downloading it on first use (cached under
/// `cache_dir()` afterwards).
pub fn load_embedder() -> astria_core::Result<TextEmbedding> {
    TextEmbedding::try_new(
        InitOptions::new(MODEL)
            .with_cache_dir(cache_dir())
            .with_show_download_progress(true),
    )
    .map_err(|e| {
        astria_core::AstriaError::Graph(format!(
            "failed to load embedding model {MODEL_NAME}: {e}"
        ))
    })
}

/// Cosine above which two nodes get a `similar_to` edge.
pub const DEFAULT_SIMILARITY_THRESHOLD: f64 = 0.80;
/// Max similar_to edges kept per node (bound edge growth on big graphs).
pub const DEFAULT_TOP_K: usize = 5;
/// Model text input cap — bge was trained on short passages.
const MAX_TEXT_CHARS: usize = 1500;
/// Below this cosine a query match is noise, not recall.
const MIN_QUERY_SIMILARITY: f64 = 0.55;
/// Semantic candidates fed into the query engine per question.
const MAX_QUERY_CANDIDATES: usize = 50;

/// Compute the embedding for one text (a question, typically).
pub fn embed_one(embedder: &mut TextEmbedding, text: &str) -> astria_core::Result<Vec<f32>> {
    embedder
        .embed(vec![text.to_string()], None)
        .map_err(|e| astria_core::AstriaError::Graph(format!("embedding failed: {e}")))
        .map(|mut v| v.remove(0))
}

/// The text a node is embedded by: label plus its best description
/// (docstring or signature), the same evidence a reader would use.
pub fn node_text(label: &str, docstring: Option<&str>, signature: Option<&str>) -> String {
    let mut text = String::from(label);
    let description = docstring
        .filter(|d| !d.trim().is_empty())
        .or_else(|| signature.filter(|s| !s.trim().is_empty()));
    if let Some(description) = description {
        text.push('\n');
        text.push_str(description.trim());
    }
    if text.len() > MAX_TEXT_CHARS {
        text.truncate(MAX_TEXT_CHARS);
    }
    text
}

/// f32 vector <-> little-endian BLOB codec for SQLite storage.
pub fn vec_to_blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub fn blob_to_vec(blob: &[u8]) -> Vec<f32> {
    let (chunks, _) = blob.as_chunks::<4>();
    chunks.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

/// Cosine similarity of two equal-length vectors (0.0 on length mismatch).
pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for i in 0..a.len() {
        dot += a[i] as f64 * b[i] as f64;
        norm_a += (a[i] as f64).powi(2);
        norm_b += (b[i] as f64).powi(2);
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// Embed every node missing from `node_embeddings` (or inserted by a
/// different model) and store the vectors. Returns the number embedded.
/// Callers pass `None` for `embedder` when they only want to refresh the
/// table with an already-loaded model — this function owns batching.
pub fn embed_missing_nodes(
    db: &Connection,
    embedder: &mut TextEmbedding,
    batch_size: usize,
) -> astria_core::Result<usize> {
    let pending: Vec<(String, String)> = {
        let mut stmt = db.prepare(
            "SELECT n.id, n.label FROM nodes n
             LEFT JOIN node_embeddings e ON e.node_id = n.id
             WHERE e.node_id IS NULL OR e.model != ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![MODEL_NAME], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.flatten().collect()
    };
    if pending.is_empty() {
        return Ok(0);
    }

    // Descriptions fetched in one pass keyed by id.
    let mut descriptions: std::collections::HashMap<String, (Option<String>, Option<String>)> =
        std::collections::HashMap::new();
    {
        let mut stmt = db.prepare("SELECT id, docstring, signature FROM nodes")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ),
            ))
        })?;
        for (id, desc) in rows.flatten() {
            descriptions.insert(id, desc);
        }
    }

    let mut embedded = 0usize;
    let batch_size = batch_size.max(1);
    for chunk in pending.chunks(batch_size) {
        let texts: Vec<String> = chunk
            .iter()
            .map(|(id, label)| {
                let (docstring, signature) = descriptions.get(id).cloned().unwrap_or((None, None));
                node_text(label, docstring.as_deref(), signature.as_deref())
            })
            .collect();
        let vectors = embedder.embed(texts, None).map_err(|e| {
            astria_core::AstriaError::Graph(format!("embedding batch failed: {e}"))
        })?;
        let tx = db.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO node_embeddings (node_id, dim, embedding, model, embedded_at)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))",
            )?;
            for ((id, _), vector) in chunk.iter().zip(vectors.iter()) {
                stmt.execute(rusqlite::params![
                    id,
                    vector.len() as i64,
                    vec_to_blob(vector),
                    MODEL_NAME
                ])?;
                embedded += 1;
            }
        }
        tx.commit()?;
    }
    Ok(embedded)
}

/// Regenerate `similar_to` edges from stored embeddings: for each node the
/// top-K most similar neighbors above the threshold, one edge per pair
/// (lexicographically smaller id first). Existing similar_to edges are
/// replaced, so re-running is idempotent. Returns edges inserted.
pub fn rebuild_similarity_edges(
    db: &Connection,
    threshold: f64,
    top_k: usize,
) -> astria_core::Result<usize> {
    let vectors: Vec<(String, Vec<f32>)> = {
        let mut stmt = db.prepare("SELECT node_id, embedding FROM node_embeddings")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                blob_to_vec(&row.get::<_, Vec<u8>>(1)?),
            ))
        })?;
        rows.flatten().collect()
    };

    db.execute("DELETE FROM edges WHERE relation = 'similar_to'", [])?;

    let mut pairs: Vec<(String, String, f64)> = Vec::new();
    for (i, (id_a, vec_a)) in vectors.iter().enumerate() {
        let mut candidates: Vec<(f64, &String)> = Vec::new();
        for (id_b, vec_b) in vectors.iter().skip(i + 1) {
            let similarity = cosine(vec_a, vec_b);
            if similarity >= threshold {
                candidates.push((similarity, id_b));
            }
        }
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        for (similarity, id_b) in candidates.into_iter().take(top_k) {
            let (source, target) = if id_a < id_b {
                (id_a.clone(), id_b.clone())
            } else {
                (id_b.clone(), id_a.clone())
            };
            pairs.push((source, target, similarity));
        }
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    pairs.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

    let tx = db.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file)
             VALUES (?1, ?2, 'similar_to', 'INFERRED', ?3, 'semantic')",
        )?;
        for (source, target, similarity) in &pairs {
            stmt.execute(rusqlite::params![source, target, similarity])?;
        }
    }
    tx.commit()?;
    Ok(pairs.len())
}

/// True when the graph has stored embeddings for the current model.
pub fn has_embeddings(db: &Connection) -> bool {
    db.query_row(
        "SELECT COUNT(*) FROM node_embeddings WHERE model = ?1",
        rusqlite::params![MODEL_NAME],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .unwrap_or(false)
}

/// Semantic candidates for a question: cosine of the question embedding
/// against every stored node embedding, mapped into the query engine's
/// seed-score scale. Only meaningfully-similar nodes are returned.
pub fn semantic_scores(
    db: &Connection,
    embedder: &mut TextEmbedding,
    question: &str,
) -> astria_core::Result<Vec<(String, f64)>> {
    let query_vector = embed_one(embedder, question)?;
    let mut stmt = db.prepare("SELECT node_id, embedding FROM node_embeddings")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            blob_to_vec(&row.get::<_, Vec<u8>>(1)?),
        ))
    })?;

    let mut scored: Vec<(String, f64)> = rows
        .flatten()
        .map(|(id, vector)| (id, cosine(&query_vector, &vector)))
        .filter(|(_, similarity)| *similarity >= MIN_QUERY_SIMILARITY)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(MAX_QUERY_CANDIDATES);
    Ok(scored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    #[test]
    fn cosine_basics() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-9);
        assert!(cosine(&a, &b).abs() < 1e-9);
        assert_eq!(cosine(&a, &[]), 0.0);
    }

    #[test]
    fn blob_roundtrip() {
        let v = vec![0.25f32, -1.5, 3.0];
        let blob = vec_to_blob(&v);
        assert_eq!(blob.len(), 12);
        assert_eq!(blob_to_vec(&blob), v);
    }

    #[test]
    fn node_text_prefers_docstring_and_caps_length() {
        assert_eq!(node_text("a()", Some("does x"), None), "a()\ndoes x");
        assert_eq!(node_text("a()", None, Some("fn a()")), "a()\nfn a()");
        let long = "x".repeat(3000);
        assert!(node_text(&long, None, None).len() <= MAX_TEXT_CHARS);
    }

    #[test]
    fn similarity_edges_from_injected_vectors() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('a', 'Alpha', 'code', 'f.rs'),
              ('b', 'Beta', 'code', 'f.rs'),
              ('c', 'Gamma', 'code', 'f.rs');
            ",
        )
        .unwrap();
        // a~b nearly identical, c orthogonal
        let a = vec![1.0f32, 0.0];
        let b = vec![0.99f32, 0.02];
        let c = vec![0.0f32, 1.0];
        for (id, v) in [("a", a), ("b", b), ("c", c)] {
            db.execute(
                "INSERT INTO node_embeddings (node_id, dim, embedding, model, embedded_at)
                 VALUES (?1, 2, ?2, ?3, datetime('now'))",
                rusqlite::params![id, vec_to_blob(&v), MODEL_NAME],
            )
            .unwrap();
        }

        let edges = rebuild_similarity_edges(&db, 0.80, 5).unwrap();
        assert_eq!(edges, 1);
        let (source, target, score): (String, String, f64) = db
            .query_row(
                "SELECT source, target, confidence_score FROM edges WHERE relation = 'similar_to'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((source.as_str(), target.as_str()), ("a", "b"));
        assert!(score > 0.99 && score <= 1.0);

        // re-running replaces, does not duplicate
        let again = rebuild_similarity_edges(&db, 0.80, 5).unwrap();
        assert_eq!(again, 1);
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE relation = 'similar_to'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    /// Real model round-trip — downloads bge-small-en-v1.5 on first run
    /// (~90 MB), offline afterwards. Ignored in normal test runs.
    #[test]
    #[ignore]
    fn real_model_embeds_and_scores() {
        let mut embedder = load_embedder().unwrap();
        let auth = embed_one(
            &mut embedder,
            "user authentication and login session handling",
        )
        .unwrap();
        let session =
            embed_one(&mut embedder, "SessionMiddleware validates session cookies").unwrap();
        let math = embed_one(&mut embedder, "matrix determinant computation").unwrap();
        let sim_auth_session = cosine(&auth, &session);
        let sim_auth_math = cosine(&auth, &math);
        assert!(
            sim_auth_session > sim_auth_math,
            "auth should be closer to session middleware ({sim_auth_session:.3}) than to matrix math ({sim_auth_math:.3})"
        );
        assert!(sim_auth_session > 0.6);
    }
}
