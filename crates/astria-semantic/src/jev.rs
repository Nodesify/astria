// Jev (TypeSafe System One) integration: a decision layer that wraps a
// completion engine. Jev does not generate text — it returns typed
// judgments (Choice / Noul / Score) with calibrated probabilities — so the
// engine backends (Claude / OpenAI-compatible / Gemini) still produce the
// node/edge JSON, and Jev improves the result in three places:
//
//   1. `gate_files`: batched keep/drop judgments over candidate files
//      BEFORE their first extraction, so trivially empty files never cost
//      an engine call (`ASTRIA_LLM_JEV_GATE`).
//   2. `verify_extraction`: per file, re-chooses relations and node types
//      from the schema allowlists and judges whether each edge is genuine
//      (`ASTRIA_LLM_JEV_VERIFY`). Chosen relations replace the lossy
//      sanitize clamps, and the keep probability becomes the edge's
//      calibrated `confidence_score` in the graph.
//   3. `rank_questions`: keep/drop scores over suggested questions so the
//      report leads with the most useful one.
//
// Wire format: `https://api.typesafe.ai/v1/systemone` with
// `questions: {id: {"type": "choice", "criteria": {id: label},
// "instructions": {...}}}` and answers of the shape
// `{choice, probabilities, confidence}`. This is the shape exercised by the
// installed jev-ultrafast harness; caps below keep every request bounded.

use crate::SemanticExtraction;
use astria_core::{AstriaError, Result};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

/// TypeSafe System One endpoint (Jev).
const SYSTEMONE_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

/// How many extracted nodes a verify request re-judges (rest stay as-is).
const MAX_VERIFY_NODES: usize = 40;
/// How many extracted edges a verify request re-judges (rest stay as-is).
const MAX_VERIFY_EDGES: usize = 40;
/// Content preview sent as evidence for the verify judgments.
const MAX_VERIFY_CONTENT_CHARS: usize = 12_000;
/// Cap on suggested questions ranked in one request.
const MAX_RANK_QUESTIONS: usize = 32;
/// Gates above this threshold are returned in a single request.
const MAX_GATE_BATCH: usize = 200;

const NODE_TYPE_GOAL: &str = "Classify this extracted node's type.";
const RELATION_GOAL: &str = "Choose the most accurate relationship for this edge.";
const EDGE_EXISTENCE_GOAL: &str = "Is this relationship genuinely present in the content?";
const GATE_GOAL: &str =
    "Does this file likely contain meaningful concepts worth semantic extraction?";
const RANK_GOAL: &str =
    "Keep questions that would genuinely help a developer understand this codebase.";

fn node_type_criteria() -> Value {
    json!({
        "concept": "Concept, topic, or theme",
        "entity": "Named entity (a concrete thing)",
        "pattern": "Reusable pattern or idiom",
        "module": "Module, package, or unit",
        "function": "Function or operation",
    })
}

fn relation_criteria() -> Value {
    json!({
        "depends_on": "Depends on or requires",
        "implements": "Implements or fulfills",
        "relates_to": "Generally related to",
        "contains": "Contains or includes",
        "uses": "Uses or references",
    })
}

/// Criteria for a yes/no judgment expressed as a two-option choice (the
/// verified Jev wire shape). The keep probability is the calibrated signal.
fn keep_drop_criteria(keep: &str, drop: &str) -> Value {
    json!({ "keep": keep, "drop": drop })
}

/// Everything about the decision layer that must invalidate cached
/// extractions when it changes: model, endpoint, toggles, thresholds, and
/// the prompt text itself.
fn prompt_fingerprint() -> String {
    format!(
        "{NODE_TYPE_GOAL}\n{RELATION_GOAL}\n{EDGE_EXISTENCE_GOAL}\n{GATE_GOAL}\n{RANK_GOAL}\n\
         {}\n{}\n{}{}",
        node_type_criteria(),
        relation_criteria(),
        keep_drop_criteria("Genuinely present", "Spurious or unsupported"),
        keep_drop_criteria("Likely meaningful", "Trivial or empty"),
    )
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct JevConfig {
    pub api_key: String,
    pub model: String,
    pub endpoint: String,
    pub verify_enabled: bool,
    /// Drop edges whose existence probability is below this (0..=1).
    pub min_edge_probability: f64,
    pub gate_enabled: bool,
    /// Files larger than this skip the gate (they are presumed rich).
    pub gate_max_bytes: u64,
    /// Drop a file when the gate's drop probability exceeds this (0..=1).
    pub gate_drop_threshold: f64,
    /// Files judged per gate request.
    pub gate_batch: usize,
}

impl JevConfig {
    /// - `ASTRIA_LLM_JUDGE_API_KEY` (or `TYPESAFE_API_KEY`) — required.
    /// - `ASTRIA_LLM_JUDGE_MODEL` — optional, defaults to `jev-latest`.
    /// - `ASTRIA_LLM_JEV_VERIFY` — on/off, default on.
    /// - `ASTRIA_LLM_JEV_MIN_EDGE_PROBABILITY` — default 0.40.
    /// - `ASTRIA_LLM_JEV_GATE` — on/off, default on.
    /// - `ASTRIA_LLM_JEV_GATE_MAX_BYTES` — default 65536.
    /// - `ASTRIA_LLM_JEV_GATE_DROP_THRESHOLD` — default 0.40.
    /// - `ASTRIA_LLM_JEV_GATE_BATCH` — default 50.
    ///
    /// Selection/config uses generic `JUDGE` names (vendor-swappable);
    /// the behavior knobs keep the honest `JEV` prefix — gate/verify
    /// semantics are System One specifics.
    pub fn from_env() -> Result<Self> {
        let api_key = astria_core::env_var("LLM_JUDGE_API_KEY")
            .or_else(|| {
                std::env::var("TYPESAFE_API_KEY")
                    .ok()
                    .filter(|v| !v.trim().is_empty())
            })
            .ok_or_else(|| {
                AstriaError::Graph(
                    "ASTRIA_LLM_JUDGE_API_KEY (or TYPESAFE_API_KEY) environment variable is not set"
                        .into(),
                )
            })?;
        Ok(Self {
            api_key,
            model: astria_core::env_var("LLM_JUDGE_MODEL").unwrap_or_else(|| "jev-latest".into()),
            endpoint: SYSTEMONE_ENDPOINT.into(),
            verify_enabled: env_bool("LLM_JEV_VERIFY", true),
            min_edge_probability: env_f64("LLM_JEV_MIN_EDGE_PROBABILITY", 0.40).clamp(0.0, 1.0),
            gate_enabled: env_bool("LLM_JEV_GATE", true),
            gate_max_bytes: env_u64("LLM_JEV_GATE_MAX_BYTES", 64 * 1024),
            gate_drop_threshold: env_f64("LLM_JEV_GATE_DROP_THRESHOLD", 0.40).clamp(0.0, 1.0),
            gate_batch: env_usize("LLM_JEV_GATE_BATCH", 50).clamp(1, MAX_GATE_BATCH),
        })
    }

    /// Non-secret identity: enough to invalidate semantic caches when the
    /// decision layer's effective configuration changes.
    pub fn identity(&self) -> String {
        format!(
            "jev model={} endpoint={} verify={} min_edge={:.3} gate={} gate_bytes={} \
             gate_drop={:.3} gate_batch={}\n{}",
            self.model,
            self.endpoint,
            self.verify_enabled,
            self.min_edge_probability,
            self.gate_enabled,
            self.gate_max_bytes,
            self.gate_drop_threshold,
            self.gate_batch,
            prompt_fingerprint()
        )
    }
}

fn env_bool(name: &str, default: bool) -> bool {
    match astria_core::env_var(name).as_deref() {
        None => default,
        Some(v) => match v.trim().to_lowercase().as_str() {
            "1" | "true" | "on" | "yes" => true,
            "0" | "false" | "off" | "no" => false,
            _ => default,
        },
    }
}

fn env_f64(name: &str, default: f64) -> f64 {
    astria_core::env_var(name)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    astria_core::env_var(name)
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    astria_core::env_var(name)
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct JevClient {
    agent: ureq::Agent,
    config: JevConfig,
}

impl JevClient {
    pub fn new(config: JevConfig) -> Self {
        Self {
            agent: crate::build_agent(),
            config,
        }
    }

    /// One System One call. Recounts usage into the shared tracker (an
    /// unmeasured call must never look free) and requires an `answers`
    /// object so malformed responses fail loudly instead of silently
    /// judging everything as "keep".
    fn judge(&self, state: &Value, questions: &Value) -> Result<Value> {
        let body = serde_json::to_string(&json!({
            "model": self.config.model,
            "state": state,
            "questions": questions,
        }))?;
        let owned_headers = vec![
            ("Content-Type", "application/json".to_string()),
            ("Authorization", format!("Bearer {}", self.config.api_key)),
        ];
        let headers: Vec<(&str, &str)> = owned_headers
            .iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();
        let response = crate::post_json(
            &self.agent,
            &self.config.endpoint,
            &headers,
            &body,
            "Jev",
        )?;
        let json: Value = serde_json::from_str(&response)
            .map_err(|e| AstriaError::Graph(format!("Failed to parse Jev response: {e}")))?;
        crate::enrichment::record_usage(&json);
        if json.get("answers").is_none() {
            let preview: String = response.chars().take(200).collect();
            return Err(AstriaError::Graph(format!(
                "Jev returned no answers: {preview}"
            )));
        }
        Ok(json)
    }

    /// Re-judge one extraction: node types, edge relations, and edge
    /// existence, in a single request. See `verify_extraction`.
    pub(crate) fn verify(
        &self,
        extraction: &SemanticExtraction,
        content: &str,
        file_type: &str,
    ) -> Result<SemanticExtraction> {
        let request = build_verify_request(extraction, content, file_type);
        let response = self.judge(&request["state"], &request["questions"])?;
        let answers = response.get("answers").cloned().unwrap_or(Value::Null);
        Ok(verify_extraction(
            extraction,
            &answers,
            self.config.min_edge_probability,
        ))
    }

    /// Gate a batch of files: batched keep/drop judgments, one request per
    /// `gate_batch` files. Files that are too large, unreadable, or missing
    /// a judgment are always kept — the gate may only save calls, never
    /// lose facts.
    pub(crate) fn gate_files(&self, files: &[PathBuf], config: &JevConfig) -> Result<Vec<PathBuf>> {
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let mut keep: Vec<bool> = vec![true; files.len()];
        let mut chunk: Vec<(usize, &PathBuf)> = Vec::new();
        for (i, file) in files.iter().enumerate() {
            let size = std::fs::metadata(file)
                .map(|m| m.len())
                .unwrap_or(u64::MAX);
            if size > config.gate_max_bytes {
                continue;
            }
            chunk.push((i, file));
            if chunk.len() >= config.gate_batch {
                self.gate_chunk(&mut keep, &chunk, config)?;
                chunk.clear();
            }
        }
        if !chunk.is_empty() {
            self.gate_chunk(&mut keep, &chunk, config)?;
        }
        Ok(files
            .iter()
            .enumerate()
            .filter(|(i, _)| keep[*i])
            .map(|(_, f)| f.clone())
            .collect())
    }

    fn gate_chunk(
        &self,
        keep: &mut Vec<bool>,
        chunk: &[(usize, &PathBuf)],
        config: &JevConfig,
    ) -> Result<()> {
        let files: Vec<Value> = chunk
            .iter()
            .map(|(_, f)| {
                let extension = f
                    .extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_default();
                let bytes = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
                json!({
                    "path": f.to_string_lossy().to_string(),
                    "extension": extension,
                    "bytes": bytes,
                })
            })
            .collect();
        let mut questions = Map::new();
        for (j, (_, f)) in chunk.iter().enumerate() {
            questions.insert(
                format!("g_{j}"),
                json!({
                    "type": "choice",
                    "criteria": keep_drop_criteria("Likely meaningful", "Trivial or empty"),
                    "instructions": {
                        "file": f.to_string_lossy().to_string(),
                        "goal": GATE_GOAL,
                    },
                }),
            );
        }
        let response = self
            .judge(&json!({ "files": files }), &Value::Object(questions))?;
        let answers = response.get("answers").cloned().unwrap_or(Value::Null);
        let decisions = gate_keeps(&answers, chunk.len(), config.gate_drop_threshold);
        for (j, (i, _)) in chunk.iter().enumerate() {
            if !decisions[j] {
                keep[*i] = false;
            }
        }
        Ok(())
    }

    /// Rank suggested questions by keep probability. Returns a permutation
    /// of `0..questions.len()` in most-useful-first order.
    pub(crate) fn rank_questions(&self, questions: &[String]) -> Result<Vec<usize>> {
        let n = questions.len();
        if n <= 1 {
            return Ok((0..n).collect());
        }
        let mut qs = Map::new();
        for (i, q) in questions.iter().take(MAX_RANK_QUESTIONS).enumerate() {
            qs.insert(
                format!("q_{i}"),
                json!({
                    "type": "choice",
                    "criteria": keep_drop_criteria(
                        "Worth a developer's attention",
                        "Not worth asking",
                    ),
                    "instructions": { "question": q, "goal": RANK_GOAL },
                }),
            );
        }
        let response = self.judge(&json!({ "kind": "suggested-question ranking" }), &Value::Object(qs))?;
        let answers = response.get("answers").cloned().unwrap_or(Value::Null);
        let mut scores: Vec<f64> = vec![0.0; n];
        for (i, _) in questions.iter().take(MAX_RANK_QUESTIONS).enumerate() {
            if let Some((keep, _)) = answers.get(&format!("q_{i}")).and_then(parse_keep_drop) {
                scores[i] = keep;
            }
        }
        Ok(order_by_scores(&scores))
    }
}

// ---------------------------------------------------------------------------
// Answer parsing (light validation; the harness's strict checks exist for
// browser-action safety, not for enrichment judgments)
// ---------------------------------------------------------------------------

/// A choice answer: `{choice, probabilities, confidence}`. Returns the
/// chosen id and its probability, both validated finite.
pub(crate) fn parse_choice_answer(answer: &Value) -> Option<(String, f64)> {
    let choice = answer.get("choice")?.as_str()?;
    if choice.trim().is_empty() {
        return None;
    }
    let probability = answer
        .get("probabilities")?
        .as_object()?
        .get(choice)?
        .as_f64()?;
    if !probability.is_finite() {
        return None;
    }
    Some((choice.to_string(), probability.clamp(0.0, 1.0)))
}

/// A keep/drop choice answer: returns `(p_keep, p_drop)`. When only one
/// side is present the other is inferred so callers can use either
/// threshold style.
pub(crate) fn parse_keep_drop(answer: &Value) -> Option<(f64, f64)> {
    let probabilities = answer.get("probabilities")?.as_object()?;
    let keep = probabilities.get("keep").and_then(|v| v.as_f64());
    let drop = probabilities.get("drop").and_then(|v| v.as_f64());
    match (keep, drop) {
        (Some(k), Some(d)) => Some((k.clamp(0.0, 1.0), d.clamp(0.0, 1.0))),
        (Some(k), None) => Some((k.clamp(0.0, 1.0), (1.0 - k).clamp(0.0, 1.0))),
        (None, Some(d)) => Some(((1.0 - d).clamp(0.0, 1.0), d.clamp(0.0, 1.0))),
        (None, None) => None,
    }
}

// ---------------------------------------------------------------------------
// Verify pass
// ---------------------------------------------------------------------------

/// Build the `{state, questions}` body for one verify request. Node types
/// (`t_<i>`), relations (`r_<i>`), and existence (`e_<i>`) are independent
/// questions over the same state, so they run in one parallel request.
/// Items beyond the caps are left untouched by `verify_extraction`. The
/// model is stamped by the client at send time.
pub(crate) fn build_verify_request(
    extraction: &SemanticExtraction,
    content: &str,
    file_type: &str,
) -> Value {
    let nodes: Vec<Value> = extraction
        .nodes
        .iter()
        .take(MAX_VERIFY_NODES)
        .map(|n| {
            json!({
                "id": n.id,
                "label": n.label,
                "node_type": n.node_type,
            })
        })
        .collect();
    let edges: Vec<Value> = extraction
        .edges
        .iter()
        .take(MAX_VERIFY_EDGES)
        .map(|e| {
            json!({
                "source": e.source,
                "target": e.target,
                "relation": e.relation,
            })
        })
        .collect();
    let mut questions = Map::new();
    for (i, node) in extraction.nodes.iter().take(MAX_VERIFY_NODES).enumerate() {
        questions.insert(
            format!("t_{i}"),
            json!({
                "type": "choice",
                "criteria": node_type_criteria(),
                "instructions": {
                    "node": format!("{} — {}", node.id, node.label),
                    "goal": NODE_TYPE_GOAL,
                },
            }),
        );
    }
    for (i, edge) in extraction.edges.iter().take(MAX_VERIFY_EDGES).enumerate() {
        let label = format!("{} -{}-> {}", edge.source, edge.relation, edge.target);
        questions.insert(
            format!("r_{i}"),
            json!({
                "type": "choice",
                "criteria": relation_criteria(),
                "instructions": { "edge": &label, "goal": RELATION_GOAL },
            }),
        );
        questions.insert(
            format!("e_{i}"),
            json!({
                "type": "choice",
                "criteria": keep_drop_criteria("Genuinely present", "Spurious or unsupported"),
                "instructions": { "edge": &label, "goal": EDGE_EXISTENCE_GOAL },
            }),
        );
    }
    let content_preview: String = content.chars().take(MAX_VERIFY_CONTENT_CHARS).collect();
    json!({
        "state": {
            "file_type": file_type,
            "content": content_preview,
            "nodes": nodes,
            "edges": edges,
        },
        "questions": questions,
    })
}

/// Apply verify answers to an extraction:
/// - node types and relations are replaced by the chosen allowlist value
///   (malformed/missing answers keep the engine's value, which sanitize
///   already clamped);
/// - edges whose existence probability is below `min_edge_probability` are
///   dropped;
/// - kept edges carry the existence probability as `confidence_score`.
pub(crate) fn verify_extraction(
    extraction: &SemanticExtraction,
    answers: &Value,
    min_edge_probability: f64,
) -> SemanticExtraction {
    let answers = answers.as_object();
    let mut out = extraction.clone();
    for (i, node) in out.nodes.iter_mut().take(MAX_VERIFY_NODES).enumerate() {
        let Some(answer) = answers.and_then(|a| a.get(&format!("t_{i}"))) else {
            continue;
        };
        if let Some((choice, _)) = parse_choice_answer(answer) {
            if crate::ALLOWED_NODE_TYPES.contains(&choice.as_str()) {
                node.node_type = choice;
            }
        }
    }
    let mut kept = Vec::with_capacity(out.edges.len());
    for (i, mut edge) in out.edges.into_iter().enumerate() {
        if i >= MAX_VERIFY_EDGES {
            kept.push(edge);
            continue;
        }
        let relation_answer = answers.and_then(|a| a.get(&format!("r_{i}")));
        let existence_answer = answers.and_then(|a| a.get(&format!("e_{i}")));
        if let Some((keep, _)) = existence_answer.and_then(parse_keep_drop) {
            if keep < min_edge_probability {
                continue;
            }
            edge.confidence_score = Some(keep);
        }
        if let Some((choice, _)) = relation_answer.and_then(parse_choice_answer) {
            if crate::ALLOWED_RELATIONS.contains(&choice.as_str()) {
                edge.relation = choice;
            }
        }
        kept.push(edge);
    }
    out.edges = kept;
    out
}

// ---------------------------------------------------------------------------
// Gate decisions
// ---------------------------------------------------------------------------

/// Per-file keep decisions for one gate request (`g_<j>` answers). A file
/// without a judgment is kept.
pub(crate) fn gate_keeps(answers: &Value, count: usize, drop_threshold: f64) -> Vec<bool> {
    (0..count)
        .map(|j| match answers.get(&format!("g_{j}")).and_then(parse_keep_drop) {
            Some((_, drop)) => drop <= drop_threshold,
            None => true,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Ranking
// ---------------------------------------------------------------------------

/// Stable permutation of `0..scores.len()` ordered by score, descending.
pub(crate) fn order_by_scores(scores: &[f64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| {
        scores[b]
            .partial_cmp(&scores[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    order
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SemanticEdge;
    use crate::SemanticNode;

    fn sample_extraction() -> SemanticExtraction {
        SemanticExtraction {
            nodes: vec![
                SemanticNode {
                    id: "a".into(),
                    label: "A".into(),
                    summary: "sum".into(),
                    node_type: "concept".into(),
                },
                SemanticNode {
                    id: "b".into(),
                    label: "B".into(),
                    summary: "sum".into(),
                    node_type: "entity".into(),
                },
            ],
            edges: vec![
                SemanticEdge {
                    source: "a".into(),
                    target: "b".into(),
                    relation: "relates_to".into(),
                    confidence_score: None,
                },
                SemanticEdge {
                    source: "b".into(),
                    target: "a".into(),
                    relation: "uses".into(),
                    confidence_score: None,
                },
            ],
        }
    }

    #[test]
    fn choice_answer_parses_and_clamps() {
        let good = json!({"choice": "uses", "probabilities": {"uses": 0.8, "contains": 0.2}, "confidence": 0.9});
        let (choice, p) = parse_choice_answer(&good).unwrap();
        assert_eq!(choice, "uses");
        assert!((p - 0.8).abs() < 1e-9);

        assert!(parse_choice_answer(&json!({"probabilities": {"x": 1.0}})).is_none());
        assert!(parse_choice_answer(&json!({"choice": "", "probabilities": {}})).is_none());
        let bad_prob = json!({"choice": "x", "probabilities": {"x": "high"}});
        assert!(parse_choice_answer(&bad_prob).is_none());
        let out_of_range = json!({"choice": "x", "probabilities": {"x": 1.7}});
        assert!((parse_choice_answer(&out_of_range).unwrap().1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn keep_drop_parses_both_and_infers_one_sided() {
        let both = json!({"choice": "keep", "probabilities": {"keep": 0.6, "drop": 0.4}});
        let (k, d) = parse_keep_drop(&both).unwrap();
        assert!((k - 0.6).abs() < 1e-9 && (d - 0.4).abs() < 1e-9);

        let keep_only = json!({"choice": "keep", "probabilities": {"keep": 0.7}});
        let (k, d) = parse_keep_drop(&keep_only).unwrap();
        assert!((k - 0.7).abs() < 1e-9 && (d - 0.3).abs() < 1e-9);
        assert!(parse_keep_drop(&json!({"choice": "x", "probabilities": {}})).is_none());
    }

    #[test]
    fn verify_replaces_relations_and_types_from_allowlist() {
        let extraction = sample_extraction();
        let answers = json!({
            "t_0": {"choice": "module", "probabilities": {"module": 0.9, "concept": 0.1}},
            "t_1": {"choice": "banana", "probabilities": {"banana": 0.9}},
            "r_0": {"choice": "depends_on", "probabilities": {"depends_on": 0.7}},
            "e_0": {"choice": "keep", "probabilities": {"keep": 0.9, "drop": 0.1}},
            "e_1": {"choice": "keep", "probabilities": {"keep": 0.2, "drop": 0.8}},
        });
        let verified = verify_extraction(&extraction, &answers, 0.40);
        assert_eq!(verified.nodes[0].node_type, "module", "valid choice applied");
        assert_eq!(
            verified.nodes[1].node_type, "entity",
            "non-allowlist choice keeps the sanitized engine value"
        );
        assert_eq!(verified.edges.len(), 1, "low-confidence edge dropped");
        assert_eq!(verified.edges[0].relation, "depends_on");
        assert_eq!(verified.edges[0].confidence_score, Some(0.9));
    }

    #[test]
    fn verify_missing_answers_keep_everything_unscored() {
        let extraction = sample_extraction();
        let verified = verify_extraction(&extraction, &Value::Null, 0.40);
        let original: Vec<(String, String, String)> = extraction
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone(), n.node_type.clone()))
            .collect();
        let after: Vec<(String, String, String)> = verified
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.label.clone(), n.node_type.clone()))
            .collect();
        assert_eq!(after, original);
        assert_eq!(verified.edges.len(), 2);
        assert!(verified.edges.iter().all(|e| e.confidence_score.is_none()));
    }

    #[test]
    fn verify_caps_bounded_questions() {
        let mut extraction = sample_extraction();
        for i in 0..50 {
            extraction.nodes.push(SemanticNode {
                id: format!("n{i}"),
                label: "N".into(),
                summary: String::new(),
                node_type: "concept".into(),
            });
        }
        extraction.edges.clear();
        for i in 0..50 {
            extraction.edges.push(SemanticEdge {
                source: "a".into(),
                target: format!("n{i}"),
                relation: "uses".into(),
                confidence_score: None,
            });
        }
        let request = build_verify_request(&extraction, "content", "rust");
        let questions = request["questions"].as_object().unwrap();
        assert_eq!(questions.len(), MAX_VERIFY_NODES + 2 * MAX_VERIFY_EDGES);
        assert!(questions.contains_key("t_39"));
        assert!(!questions.contains_key("t_40"));
        assert!(!questions.contains_key("r_40"));
        assert!(questions.get("r_0").unwrap()["type"] == "choice");
        assert!(questions.get("e_0").unwrap()["criteria"]["drop"].is_string());
        assert_eq!(request["state"]["content"], "content");
        assert!(request["state"]["nodes"].as_array().unwrap().len() == MAX_VERIFY_NODES);
    }

    #[test]
    fn gate_decisions_keep_missing_and_under_threshold() {
        let answers = json!({
            "g_0": {"choice": "drop", "probabilities": {"keep": 0.1, "drop": 0.9}},
            "g_1": {"choice": "keep", "probabilities": {"keep": 0.8, "drop": 0.2}},
        });
        let decisions = gate_keeps(&answers, 3, 0.40);
        assert_eq!(decisions, vec![false, true, true]);
    }

    #[test]
    fn ranking_orders_by_keep_probability_stable() {
        let scores = vec![0.2, 0.9, 0.9, 0.5];
        let order = order_by_scores(&scores);
        assert_eq!(order, vec![1, 2, 3, 0], "ties keep input order");
    }

    #[test]
    fn config_defaults_with_clean_env() {
        let _guard = crate::ENV_LOCK.lock().unwrap();
        for name in [
            "ASTRIA_LLM_JUDGE_API_KEY",
            "ASTRIA_LLM_JUDGE_MODEL",
            "ASTRIA_LLM_JEV_VERIFY",
            "ASTRIA_LLM_JEV_MIN_EDGE_PROBABILITY",
            "ASTRIA_LLM_JEV_GATE",
            "ASTRIA_LLM_JEV_GATE_MAX_BYTES",
            "ASTRIA_LLM_JEV_GATE_DROP_THRESHOLD",
            "ASTRIA_LLM_JEV_GATE_BATCH",
        ] {
            std::env::remove_var(name);
        }
        std::env::set_var("TYPESAFE_API_KEY", "k");
        let config = JevConfig::from_env().unwrap();
        assert_eq!(config.model, "jev-latest");
        assert!(config.verify_enabled && config.gate_enabled);
        assert!((config.min_edge_probability - 0.40).abs() < 1e-9);
        assert_eq!(config.gate_max_bytes, 64 * 1024);
        assert_eq!(config.gate_batch, 50);
        assert!(config.identity().contains("model=jev-latest"));
        std::env::remove_var("TYPESAFE_API_KEY");
    }

    #[test]
    fn config_requires_key() {
        let _guard = crate::ENV_LOCK.lock().unwrap();
        std::env::remove_var("ASTRIA_LLM_JUDGE_API_KEY");
        std::env::remove_var("TYPESAFE_API_KEY");
        assert!(JevConfig::from_env().is_err());
    }

    #[test]
    fn config_parses_toggles_and_clamps() {
        let _guard = crate::ENV_LOCK.lock().unwrap();
        std::env::set_var("TYPESAFE_API_KEY", "k");
        std::env::set_var("ASTRIA_LLM_JEV_VERIFY", "off");
        std::env::set_var("ASTRIA_LLM_JEV_GATE", "false");
        std::env::set_var("ASTRIA_LLM_JEV_MIN_EDGE_PROBABILITY", "2.0");
        std::env::set_var("ASTRIA_LLM_JEV_GATE_BATCH", "9999");
        let config = JevConfig::from_env().unwrap();
        assert!(!config.verify_enabled && !config.gate_enabled);
        assert!((config.min_edge_probability - 1.0).abs() < 1e-9);
        assert_eq!(config.gate_batch, MAX_GATE_BATCH);
        for name in [
            "ASTRIA_LLM_JEV_VERIFY",
            "ASTRIA_LLM_JEV_GATE",
            "ASTRIA_LLM_JEV_MIN_EDGE_PROBABILITY",
            "ASTRIA_LLM_JEV_GATE_BATCH",
        ] {
            std::env::remove_var(name);
        }
        std::env::remove_var("TYPESAFE_API_KEY");
    }

    /// Live round-trip against api.typesafe.ai covering all three passes
    /// (gate, verify, rank). Ignored by default and skipped without a key —
    /// every decision is a billed request:
    ///
    /// ```text
    /// TYPESAFE_API_KEY=... cargo test -p astria-semantic jev_live -- --ignored --nocapture
    /// ```
    ///
    /// Asserts only structural validity (the model is non-deterministic);
    /// the actual judgments print for human review.
    #[test]
    #[ignore = "billed live API call; set TYPESAFE_API_KEY and run with --ignored"]
    fn jev_live_roundtrip() {
        let Ok(config) = JevConfig::from_env() else {
            eprintln!("skipping: TYPESAFE_API_KEY / ASTRIA_LLM_JUDGE_API_KEY not set");
            return;
        };
        let client = JevClient::new(config.clone());
        eprintln!("model: {} endpoint: {}", config.model, config.endpoint);

        // -- gate: one real file vs one empty file --
        let dir = tempfile::tempdir().unwrap();
        let rich = dir.path().join("service.rs");
        std::fs::write(
            &rich,
            "pub struct UserService;\nimpl UserService {\n    pub fn connect(db: &Db) -> Session { db.open() }\n}\n",
        )
        .unwrap();
        let empty = dir.path().join("empty.txt");
        std::fs::write(&empty, "").unwrap();
        let kept = client.gate_files(&[rich.clone(), empty.clone()], &config).unwrap();
        eprintln!(
            "gate: kept {:?} (empty.txt gated out: {})",
            kept.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect::<Vec<_>>(),
            !kept.iter().any(|p| p == &empty),
        );
        assert!(kept.len() <= 2, "gate never invents files");

        // -- verify: a small synthetic extraction re-judged by Jev --
        let extraction = SemanticExtraction {
            nodes: vec![
                SemanticNode {
                    id: "user_service".into(),
                    label: "UserService".into(),
                    summary: "Opens database sessions".into(),
                    node_type: "module".into(),
                },
                SemanticNode {
                    id: "session_management".into(),
                    label: "Session management".into(),
                    summary: "Pattern of opening and closing DB sessions".into(),
                    node_type: "pattern".into(),
                },
            ],
            edges: vec![
                SemanticEdge {
                    source: "user_service".into(),
                    target: "session_management".into(),
                    relation: "relates_to".into(),
                    confidence_score: None,
                },
                SemanticEdge {
                    source: "session_management".into(),
                    target: "user_service".into(),
                    relation: "depends_on".into(),
                    confidence_score: None,
                },
            ],
        };
        let content = "pub struct UserService;\nimpl UserService {\n    /// Opens a session for each request.\n    pub fn connect(db: &Db) -> Session { db.open() }\n}\n";
        let verified = client.verify(&extraction, content, "rust").unwrap();
        for (i, edge) in verified.edges.iter().enumerate() {
            assert!(
                crate::ALLOWED_RELATIONS.contains(&edge.relation.as_str()),
                "relation must stay on the allowlist, got '{}'",
                edge.relation
            );
            if let Some(p) = edge.confidence_score {
                assert!((0.0..=1.0).contains(&p), "score must be a probability");
                eprintln!(
                    "verify edge {i} ({} -> {}): relation '{}', confidence {p:.2}",
                    edge.source, edge.target, edge.relation
                );
            } else {
                eprintln!(
                    "verify edge {i} ({} -> {}): relation '{}', no existence answer",
                    edge.source, edge.target, edge.relation
                );
            }
        }
        for (i, node) in verified.nodes.iter().enumerate() {
            assert!(
                crate::ALLOWED_NODE_TYPES.contains(&node.node_type.as_str()),
                "node_type must stay on the allowlist"
            );
            eprintln!("verify node {i} ({}): type '{}'", node.id, node.node_type);
        }

        // -- rank: three suggested questions, one clearly most useful --
        let questions = vec![
            "Why does astria_query::score_nodes have the most connections?".to_string(),
            "What is the capital of France?".to_string(),
            "Is src/legacy/old_parser.rs still used? Nothing references it.".to_string(),
        ];
        let perm = client.rank_questions(&questions).unwrap();
        let mut sorted = perm.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2], "must be a permutation");
        eprintln!(
            "rank order: {:?} (best first: {:?})",
            perm,
            perm.iter().map(|&i| &questions[i]).collect::<Vec<_>>()
        );
    }
}