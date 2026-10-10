//! Actual stage timings, with unknown memory measurements left null.
use super::*;
use std::collections::BTreeMap;
use std::time::Instant;

pub(super) struct Performance {
    started: Instant,
    checkpoint: Instant,
    stages: BTreeMap<String, u64>,
    pub reused_derived: bool,
}
impl Performance {
    pub(super) fn new() -> Self {
        Self {
            started: Instant::now(),
            checkpoint: Instant::now(),
            stages: BTreeMap::new(),
            reused_derived: false,
        }
    }
    pub(super) fn record(&mut self, stage: &str) {
        self.stages
            .insert(stage.into(), self.checkpoint.elapsed().as_micros() as u64);
        self.checkpoint = Instant::now();
    }
    pub(super) fn save(
        &mut self,
        directory: &Path,
        db: &Connection,
        successful: bool,
    ) -> astria_core::Result<()> {
        self.record("completion");
        let source_bytes: i64 = db.query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM file_manifest",
            [],
            |r| r.get(0),
        )?;
        let peak_resident_bytes = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines().find_map(|line| {
                    line.strip_prefix("VmHWM:")
                        .and_then(|value| value.split_whitespace().next()?.parse::<u64>().ok())
                        .map(|kb| kb * 1024)
                })
            });
        let report = serde_json::json!({
            "schemaVersion": 1, "successful": successful, "reusedDerived": self.reused_derived,
            "totalMicroseconds": self.started.elapsed().as_micros() as u64,
            "stageMicroseconds": self.stages, "sourceBytes": source_bytes,
            "databaseBytes": std::fs::metadata(directory.join("db.sqlite")).ok().map(|m| m.len()),
            "peakResidentBytes": peak_resident_bytes,
            "memoryNote": "Peak resident memory is available on Linux; other platforms report null. Database bytes exclude WAL files.",
        });
        write_artifact_atomic(
            &directory.join("performance.json"),
            &serde_json::to_vec_pretty(&report)?,
        )
    }
}

pub(super) fn derived_key(
    db: &Connection,
    options: &PipelineOptions,
) -> astria_core::Result<String> {
    let generation: String = db.query_row(
        "SELECT COALESCE((SELECT value FROM _meta WHERE key = 'graph_generation'), '')",
        [],
        |r| r.get(0),
    )?;
    let profile = serde_json::to_string(&profile::IndexingProfile::capture(options)?)?;
    let mut statement = db.prepare(
        "SELECT source, target, question, hits FROM query_pairs ORDER BY source, target, question",
    )?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(semantic_pass::fingerprint(&[
        "derived-v1",
        &generation,
        &profile,
        &serde_json::to_string(&rows)?,
        options.cli_version.unwrap_or(""),
    ]))
}

pub(super) fn stored_cluster(
    db: &Connection,
) -> astria_core::Result<astria_cluster::ClusterResult> {
    let mut statement = db.prepare("SELECT id, label, size FROM communities")?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, u32>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, usize>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let nodes: i64 = db.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
    let modularity = if nodes == 0 {
        0.0
    } else {
        db.query_row(
            "SELECT value FROM _meta WHERE key = 'last_modularity'",
            [],
            |r| r.get::<_, String>(0),
        )?
        .parse::<f64>()
        .map_err(|e| astria_core::AstriaError::Graph(e.to_string()))?
    };
    Ok(astria_cluster::ClusterResult {
        communities: rows.iter().map(|(id, _, size)| (*id, *size)).collect(),
        labels: rows.into_iter().map(|(id, label, _)| (id, label)).collect(),
        iterations: 0,
        modularity,
        excluded_hubs: 0,
    })
}
