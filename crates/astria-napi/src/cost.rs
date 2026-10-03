// cost.json: the surfaced cost report. Every pipeline run already records
// its measured LLM spend (llm_input_tokens / llm_output_tokens /
// llm_api_calls) in pipeline_runs; until now that data only lived in
// SQLite where nothing read it. This writes `.astria/cost.json` after each
// run — this run's spend, the cumulative total across runs, the backend
// identity, and (when the operator supplies per-million-token prices) a
// dollar estimate — so teams can diff spend across runs and wire budgets
// into CI without opening the database.

use std::path::Path;

use rusqlite::Connection;
use serde_json::json;

use astria_core::Result;

/// One row of the pipeline_runs table, as the cost report consumes it.
struct RunRow {
    started_at: String,
    finished_at: Option<String>,
    status: String,
    files_processed: Option<i64>,
    nodes_added: Option<i64>,
    edges_added: Option<i64>,
    input_tokens: i64,
    output_tokens: i64,
    api_calls: i64,
}

fn fetch_run(db: &Connection, run_id: i64) -> Result<RunRow> {
    db.query_row(
        "SELECT started_at, finished_at, status, files_processed, nodes_added, edges_added,
                COALESCE(llm_input_tokens, 0), COALESCE(llm_output_tokens, 0), COALESCE(llm_api_calls, 0)
         FROM pipeline_runs WHERE id = ?1",
        [run_id],
        |r| {
            Ok(RunRow {
                started_at: r.get(0)?,
                finished_at: r.get(1)?,
                status: r.get(2)?,
                files_processed: r.get(3)?,
                nodes_added: r.get(4)?,
                edges_added: r.get(5)?,
                input_tokens: r.get(6)?,
                output_tokens: r.get(7)?,
                api_calls: r.get(8)?,
            })
        },
    )
    .map_err(astria_core::AstriaError::from)
}

/// Cumulative totals across completed runs — the lifetime spend of this
/// graph, the number a budget alert would threshold on.
fn fetch_cumulative(db: &Connection) -> Result<(i64, i64, i64, i64)> {
    db.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(llm_input_tokens), 0),
                COALESCE(SUM(llm_output_tokens), 0),
                COALESCE(SUM(llm_api_calls), 0)
         FROM pipeline_runs WHERE status = 'completed'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .map_err(astria_core::AstriaError::from)
}

/// Optional USD-per-million-token prices; both must be set for an estimate
/// to appear (a half-priced estimate would be a lie).
fn pricing() -> Option<(f64, f64)> {
    let input =
        astria_core::env_var("COST_INPUT_PER_MTOK").and_then(|v| v.trim().parse::<f64>().ok())?;
    let output =
        astria_core::env_var("COST_OUTPUT_PER_MTOK").and_then(|v| v.trim().parse::<f64>().ok())?;
    Some((input, output))
}

fn dollars(tokens: i64, per_mtok: f64) -> f64 {
    tokens as f64 / 1_000_000.0 * per_mtok
}

/// Write `.astria/cost.json` for run `run_id`. Called after the run row is
/// finalized, so the cumulative sums include this run.
pub fn write_cost_report(
    astria_dir: &Path,
    db: &Connection,
    run_id: i64,
) -> Result<std::path::PathBuf> {
    let run = fetch_run(db, run_id)?;
    let (runs, cum_input, cum_output, cum_calls) = fetch_cumulative(db)?;

    let backend = std::env::var("ASTRIA_LLM_BACKEND")
        .ok()
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty() && !b.eq_ignore_ascii_case("none"));
    let model = astria_core::env_var("LLM_MODEL")
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());

    let mut report = json!({
        "run_id": run_id,
        "run": {
            "started_at": run.started_at,
            "finished_at": run.finished_at,
            "status": run.status,
            "files_processed": run.files_processed,
            "nodes_added": run.nodes_added,
            "edges_added": run.edges_added,
            "input_tokens": run.input_tokens,
            "output_tokens": run.output_tokens,
            "api_calls": run.api_calls,
        },
        "cumulative": {
            "runs": runs,
            "input_tokens": cum_input,
            "output_tokens": cum_output,
            "api_calls": cum_calls,
        },
        "backend": backend,
        "model": model,
    });

    if let Some((price_in, price_out)) = pricing() {
        report["estimated_cost_usd"] = json!({
            "run": dollars(run.input_tokens, price_in) + dollars(run.output_tokens, price_out),
            "cumulative": dollars(cum_input, price_in) + dollars(cum_output, price_out),
            "input_per_mtok": price_in,
            "output_per_mtok": price_out,
            "note": "tokens × operator-supplied prices (ASTRIA_COST_*_PER_MTOK); not vendor billing data",
        });
    }

    let out = astria_dir.join("cost.json");
    std::fs::write(&out, serde_json::to_vec_pretty(&report)?)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    /// The pricing knob is process-global env state; tests that touch it
    /// must not interleave.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn seeded_db() -> Connection {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO pipeline_runs (started_at, finished_at, status, files_processed, nodes_added, edges_added, llm_input_tokens, llm_output_tokens, llm_api_calls)
             VALUES ('10', '12', 'completed', 3, 5, 7, 100, 20, 2),
                    ('20', '22', 'completed', 1, 0, 0, 50, 10, 1);",
        )
        .unwrap();
        db
    }

    #[test]
    fn report_includes_run_and_cumulative() {
        let _guard = ENV_LOCK.lock().unwrap();
        for var in ["ASTRIA_COST_INPUT_PER_MTOK", "ASTRIA_COST_OUTPUT_PER_MTOK"] {
            std::env::remove_var(var);
        }
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        let out = write_cost_report(dir.path(), &db, 2).unwrap();
        let report: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap();
        assert_eq!(report["run_id"], 2);
        assert_eq!(report["run"]["input_tokens"], 50);
        assert_eq!(report["cumulative"]["runs"], 2);
        assert_eq!(report["cumulative"]["input_tokens"], 150);
        assert_eq!(report["cumulative"]["output_tokens"], 30);
        assert_eq!(report["cumulative"]["api_calls"], 3);
        // No prices supplied → no estimate, never a half-priced guess.
        assert!(report.get("estimated_cost_usd").is_none());
    }

    #[test]
    fn pricing_envs_produce_estimate() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db = seeded_db();
        std::env::set_var("ASTRIA_COST_INPUT_PER_MTOK", "3.0");
        std::env::set_var("ASTRIA_COST_OUTPUT_PER_MTOK", "15.0");
        let out = write_cost_report(dir.path(), &db, 1).unwrap();
        std::env::remove_var("ASTRIA_COST_INPUT_PER_MTOK");
        std::env::remove_var("ASTRIA_COST_OUTPUT_PER_MTOK");
        let report: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap();
        // run 1: 100 in × 3/M + 20 out × 15/M = 0.0003 + 0.0003
        let est = report["estimated_cost_usd"]["run"].as_f64().unwrap();
        assert!((est - 0.0006).abs() < 1e-9, "got {est}");
        // cumulative: 150 in + 30 out → 0.00045 + 0.00045
        let cum = report["estimated_cost_usd"]["cumulative"].as_f64().unwrap();
        assert!((cum - 0.0009).abs() < 1e-9, "got {cum}");
    }

    #[test]
    fn failed_runs_are_reported_with_their_status() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO pipeline_runs (started_at, finished_at, status, llm_input_tokens, llm_output_tokens, llm_api_calls)
             VALUES ('30', '31', 'failed', 5, 1, 1);",
        )
        .unwrap();
        let out = write_cost_report(dir.path(), &db, 1).unwrap();
        let report: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap();
        assert_eq!(report["run"]["status"], "failed");
    }
}
