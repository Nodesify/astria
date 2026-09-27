// Token-reduction benchmark: corpus tokens vs the tokens a graph query
// actually returns. Measured honestly — the corpus side sums the manifest's
// real file sizes, the query side runs this project's own query engine on
// sample questions and counts the answer text. Ported from upstream
// astria benchmark.py.

use std::path::Path;

use rusqlite::Connection;

use astria_core::Result;

/// Standard approximation: ~4 characters per token.
const CHARS_PER_TOKEN: usize = 4;
/// Traversal depth for sample queries (matches the query command default + 1).
const SAMPLE_DEPTH: usize = 3;
/// Budget high enough that truncation does not skew the measurement.
const SAMPLE_BUDGET: i64 = 4000;

const SAMPLE_QUESTIONS: [&str; 5] = [
    "how does authentication work",
    "what is the main entry point",
    "how are errors handled",
    "data layer api",
    "core abstractions",
];

pub struct BenchmarkResult {
    pub corpus_tokens: usize,
    pub corpus_bytes: usize,
    pub node_count: usize,
    pub edge_count: usize,
    pub avg_query_tokens: usize,
    pub reduction_ratio: f64,
    /// (question, query_tokens) for the questions that matched anything.
    pub per_question: Vec<(String, usize)>,
}

fn estimate_tokens(text: &str) -> usize {
    text.len() / CHARS_PER_TOKEN
}

/// Sum the manifest's recorded file sizes — the corpus a naive agent would
/// have to read in full.
fn corpus_bytes(db: &Connection) -> Result<usize> {
    let mut stmt = db.prepare("SELECT COALESCE(SUM(size_bytes), 0) FROM file_manifest")?;
    let bytes: i64 = stmt.query_row([], |row| row.get(0))?;
    Ok(bytes.max(0) as usize)
}

pub fn run_benchmark(db: &Connection, db_path: &str) -> Result<BenchmarkResult> {
    let corpus_bytes = corpus_bytes(db)?;
    let corpus_tokens = corpus_bytes / CHARS_PER_TOKEN;

    let (node_count, edge_count): (usize, usize) = {
        let mut stmt =
            db.prepare("SELECT (SELECT COUNT(*) FROM nodes), (SELECT COUNT(*) FROM edges)")?;
        let row = stmt.query_row([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
        (row.0 as usize, row.1 as usize)
    };

    // Measure what a query actually costs: run each sample question through
    // the real engine and count the returned answer.
    let mut per_question = Vec::new();
    for question in SAMPLE_QUESTIONS {
        let (text, matched, _, _) = astria_query::query_graph(
            db,
            db_path,
            question,
            "bfs",
            SAMPLE_DEPTH,
            SAMPLE_BUDGET,
            false,
            0.0,
            0,
            false,
        )?;
        if matched > 0 {
            per_question.push((question.to_string(), estimate_tokens(&text)));
        }
    }

    let avg_query_tokens = if per_question.is_empty() {
        0
    } else {
        per_question.iter().map(|(_, t)| t).sum::<usize>() / per_question.len()
    };
    let reduction_ratio = if avg_query_tokens > 0 {
        corpus_tokens as f64 / avg_query_tokens as f64
    } else {
        0.0
    };

    Ok(BenchmarkResult {
        corpus_tokens,
        corpus_bytes,
        node_count,
        edge_count,
        avg_query_tokens,
        reduction_ratio,
        per_question,
    })
}

/// Renders the benchmark block the CLI prints after a pipeline run.
pub fn render_report(result: &BenchmarkResult) -> String {
    if result.avg_query_tokens == 0 {
        return String::new();
    }
    let mut out = format!(
        "\nToken reduction benchmark\n──────────────────────────\n  Corpus:         {:.1} MB → ~{} tokens (naive full read)\n  Graph:          {} nodes, {} edges\n  Avg query cost: ~{} tokens\n  Reduction:      {:.1}x (graph query vs naive full read)\n",
        result.corpus_bytes as f64 / 1_048_576.0,
        result.corpus_tokens,
        result.node_count,
        result.edge_count,
        result.avg_query_tokens,
        result.reduction_ratio,
    );
    out.push_str(
        "  Estimator:      4 chars/token heuristic — the published snapshot also\n  reports exact o200k_base counts for both tools (token_parity block)\n",
    );
    if result.reduction_ratio < 1.5 {
        out.push_str(
            "  (small corpus — the graph's value here is structure, not compression;\n   reduction grows with corpus size)\n",
        );
    }
    out.push_str("\n  Per question:\n");
    for (question, tokens) in &result.per_question {
        let ratio = if *tokens > 0 {
            result.corpus_tokens as f64 / *tokens as f64
        } else {
            0.0
        };
        out.push_str(&format!("    [{ratio:.1}x] {question}\n"));
    }
    out
}

/// Convenience wrapper: load the graph db under `root`, run the benchmark,
/// return the rendered block (empty when no sample question matched).
pub fn benchmark_for_root(root: &Path) -> Result<String> {
    let db = crate::pipeline::load_graph_db(root)?;
    let db_path = root.join(".astria").join("db.sqlite");
    let db_path_str = db_path.to_string_lossy().to_string();
    let result = run_benchmark(&db, &db_path_str)?;
    Ok(render_report(&result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    fn seeded_db() -> Connection {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('auth', 'authenticate()', 'code', 'src/auth.rs'),
              ('err', 'handle_error()', 'code', 'src/err.rs'),
              ('entry', 'main()', 'code', 'src/main.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
              ('entry', 'auth', 'calls', 'EXTRACTED', 'src/main.rs'),
              ('auth', 'err', 'calls', 'EXTRACTED', 'src/auth.rs');
            INSERT INTO file_manifest (file_path, content_hash, file_type, last_seen_at, size_bytes)
              VALUES ('src/auth.rs', 'h1', 'code', '2026-01-01', 4000),
                     ('src/err.rs', 'h2', 'code', '2026-01-01', 8000);
            ",
        )
        .unwrap();
        db
    }

    #[test]
    fn benchmark_measures_corpus_and_queries() {
        let db = seeded_db();
        let result = run_benchmark(&db, ":memory:").unwrap();
        // 12000 bytes / 4 = 3000 corpus tokens
        assert_eq!(result.corpus_tokens, 3000);
        assert_eq!(result.node_count, 3);
        assert_eq!(result.edge_count, 2);
        assert!(!result.per_question.is_empty());
        assert!(result.avg_query_tokens > 0);
        assert!(result.reduction_ratio > 1.0);
        let report = render_report(&result);
        assert!(report.contains("3000 tokens"));
        assert!(report.contains("graph query vs naive full read"));
        // 3000 corpus tokens vs ~a few hundred query tokens on the seeded graph
        assert!(report.contains("(small corpus") || result.reduction_ratio >= 1.5);
    }

    #[test]
    fn render_report_empty_when_nothing_matches() {
        let result = BenchmarkResult {
            corpus_tokens: 0,
            corpus_bytes: 0,
            node_count: 0,
            edge_count: 0,
            avg_query_tokens: 0,
            reduction_ratio: 0.0,
            per_question: Vec::new(),
        };
        assert_eq!(render_report(&result), "");
    }
}
