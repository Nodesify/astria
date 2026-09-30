// Calibration bench for the semantic layer's cosine scale. Cosine values
// are model-specific: the same "related" level sits at a different cosine
// for bge-small-en-v1.5 than for jina-embeddings-v2-base-code, so every
// raw-cosine threshold (query noise floor, seed-rescale anchors, the
// similar_to edge threshold) must be measured, not inherited across a
// model swap. This example measures both models on one real graph:
//
//   query→node  golden questions vs every node — best cosine among the
//               expected file's nodes (the signal) and the distribution
//               over the rest (the noise floor's neighborhood)
//   node→node   nearest-neighbor cosines and pair counts at candidate
//               thresholds (whether similar_to edges survive the swap)
//
// Run against a graph built with the current model:
//   cargo run --release -p astria-embed --example calibrate -- \
//     bench-work/embed-calib/click/.astria/db.sqlite \
//     bench-work/embed-calib/questions.tsv
//
// The questions file is TSV: split, id, question, expected|files. Only
// already-exercised golden splits belong here — reserved sets must not be
// burned on calibration. bge vectors are embedded fresh (the graph only
// carries the current model's rows).

use astria_embed::{blob_to_vec, cache_dir, cosine, embed_one, node_text};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use rusqlite::Connection;

fn load(model: EmbeddingModel) -> TextEmbedding {
    let name = format!("{model:?}");
    TextEmbedding::try_new(InitOptions::new(model).with_cache_dir(cache_dir()))
        .unwrap_or_else(|e| panic!("model {name} not cached: {e}"))
}

struct NodeRow {
    id: String,
    source_file: String,
    text: String,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn main() {
    let mut args = std::env::args().skip(1);
    let db_path = args
        .next()
        .expect("usage: calibrate <db.sqlite> <questions.tsv>");
    let questions_path = args
        .next()
        .expect("usage: calibrate <db.sqlite> <questions.tsv>");

    let db = Connection::open(&db_path).expect("open graph db");
    let nodes: Vec<NodeRow> = {
        let mut stmt = db
            .prepare("SELECT id, source_file, label, docstring, signature FROM nodes")
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok(NodeRow {
                    id: row.get(0)?,
                    source_file: row.get(1)?,
                    text: node_text(
                        &row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?.as_deref(),
                        row.get::<_, Option<String>>(4)?.as_deref(),
                    ),
                })
            })
            .unwrap();
        rows.flatten().collect()
    };
    eprintln!("{} nodes", nodes.len());

    // Question rows: (split, id, question, expected files).
    let questions: Vec<(String, String, String, Vec<String>)> =
        std::fs::read_to_string(&questions_path)
            .expect("read questions")
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let mut parts = l.split('\t');
                let split = parts.next().unwrap_or("").to_string();
                let id = parts.next().unwrap_or("").to_string();
                let question = parts.next().unwrap_or_default().to_string();
                let expected = parts
                    .next()
                    .unwrap_or_default()
                    .split('|')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect();
                (split, id, question, expected)
            })
            .collect();
    eprintln!("{} questions", questions.len());

    let models: Vec<(&str, EmbeddingModel, Vec<Vec<f32>>)> = vec![
        // (label, model, node vectors) — jina from the stored rows, bge embedded fresh.
        {
            let mut stmt = db
                .prepare("SELECT node_id, embedding FROM node_embeddings WHERE model = ?1")
                .unwrap();
            let stored: Vec<(String, Vec<f32>)> = stmt
                .query_map(rusqlite::params![astria_embed::MODEL_NAME], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        blob_to_vec(&row.get::<_, Vec<u8>>(1)?),
                    ))
                })
                .unwrap()
                .flatten()
                .collect();
            let by_id: std::collections::HashMap<&str, &Vec<f32>> =
                stored.iter().map(|(id, v)| (id.as_str(), v)).collect();
            let vectors: Vec<Vec<f32>> = nodes
                .iter()
                .map(|n| {
                    by_id
                        .get(n.id.as_str())
                        .map(|v| (*v).clone())
                        .unwrap_or_default()
                })
                .collect();
            let missing = vectors.iter().filter(|v| v.is_empty()).count();
            eprintln!(
                "jina: {} stored vectors, {} nodes missing",
                stored.len(),
                missing
            );
            ("jina-v2-base-code", astria_embed::MODEL, vectors)
        },
        {
            let mut embedder = load(EmbeddingModel::BGESmallENV15);
            eprintln!("embedding {} node texts with bge-small...", nodes.len());
            let mut vectors = Vec::with_capacity(nodes.len());
            for chunk in nodes.chunks(64) {
                let texts: Vec<String> = chunk.iter().map(|n| n.text.clone()).collect();
                let out = embedder.embed(texts, None).expect("bge embed batch");
                vectors.extend(out);
            }
            ("bge-small-en-v1.5", EmbeddingModel::BGESmallENV15, vectors)
        },
    ];

    println!("== query -> node cosines (golden questions) ==");
    println!(
        "{:<9} {:<16} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "model", "question", "best-rel", "noise-p50", "noise-p95", "noise-p99", "noise-max"
    );
    for (label, model, vectors) in &models {
        let mut embedder = load(model.clone());
        for (split, id, question, expected) in &questions {
            let qv = embed_one(&mut embedder, question).expect("embed question");
            let mut relevant: Vec<f64> = Vec::new();
            let mut noise: Vec<f64> = Vec::new();
            for (node, vector) in nodes.iter().zip(vectors.iter()) {
                if vector.is_empty() {
                    continue;
                }
                let c = cosine(&qv, vector);
                // Stored source paths are absolute; expected files are
                // corpus-relative — match on a normalized path suffix.
                let normalized = node.source_file.replace('\\', "/");
                let is_relevant = expected
                    .iter()
                    .any(|e| normalized == *e || normalized.ends_with(&format!("/{e}")));
                if is_relevant {
                    relevant.push(c);
                } else {
                    noise.push(c);
                }
            }
            noise.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let best_rel = relevant.iter().cloned().fold(f64::MIN, f64::max);
            println!(
                "{:<9} {:<16} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3}  [{split}]",
                label,
                id,
                best_rel,
                percentile(&noise, 0.50),
                percentile(&noise, 0.95),
                percentile(&noise, 0.99),
                noise.last().copied().unwrap_or(0.0),
            );
        }
    }

    println!("\n== node -> node nearest-neighbor cosines ==");
    for (label, _model, vectors) in &models {
        let n = nodes.len();
        let mut top1: Vec<f64> = Vec::with_capacity(n);
        let thresholds = [0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90];
        let mut counts = vec![0usize; thresholds.len()];
        for i in 0..n {
            if vectors[i].is_empty() {
                continue;
            }
            let mut best = f64::MIN;
            for j in 0..n {
                if i == j || vectors[j].is_empty() {
                    continue;
                }
                let c = cosine(&vectors[i], &vectors[j]);
                if c > best {
                    best = c;
                }
                for (t, count) in thresholds.iter().zip(counts.iter_mut()) {
                    if i < j && c >= *t {
                        *count += 1;
                    }
                }
            }
            if best > f64::MIN {
                top1.push(best);
            }
        }
        top1.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "{:<18} top1 p50={:.3} p90={:.3} p99={:.3} max={:.3}",
            label,
            percentile(&top1, 0.50),
            percentile(&top1, 0.90),
            percentile(&top1, 0.99),
            top1.last().copied().unwrap_or(0.0),
        );
        let total_pairs = n * (n - 1) / 2;
        print!("{:<18} pairs>=", label);
        for (t, count) in thresholds.iter().zip(&counts) {
            print!(
                " {t:.2}:{count} ({:.2}%)",
                100.0 * *count as f64 / total_pairs as f64
            );
        }
        println!();
    }
}
