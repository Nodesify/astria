// Auxiliary LLM passes on top of the extraction backend: token accounting
// with an optional hard budget, thematic community naming, and deep
// concept linking. Every pass goes through `SemanticBackend::complete`
// (one single-shot call, no chunking) and every response is recorded via
// `record_usage`, so a run's token spend is always measurable and
// budget-cappable — enrichment depth without unbounded cost.

use crate::SemanticBackend;
use astria_core::{AstriaError, Result};
use std::sync::atomic::{AtomicU64, Ordering};

static USAGE_INPUT: AtomicU64 = AtomicU64::new(0);
static USAGE_OUTPUT: AtomicU64 = AtomicU64::new(0);
static USAGE_CALLS: AtomicU64 = AtomicU64::new(0);

/// Hard ceiling on input+output tokens for one pipeline run
/// (`ASTRIA_LLM_BUDGET`); 0 means unlimited.
static BUDGET: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub calls: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input + self.output
    }
}

/// Zero the per-process counters. The CLI pipeline is one run per process,
/// but tests (and any future long-lived host) need a clean slate.
pub fn reset_usage() {
    USAGE_INPUT.store(0, Ordering::Relaxed);
    USAGE_OUTPUT.store(0, Ordering::Relaxed);
    USAGE_CALLS.store(0, Ordering::Relaxed);
}

pub fn usage_snapshot() -> TokenUsage {
    TokenUsage {
        input: USAGE_INPUT.load(Ordering::Relaxed),
        output: USAGE_OUTPUT.load(Ordering::Relaxed),
        calls: USAGE_CALLS.load(Ordering::Relaxed),
    }
}

/// Record one API response's usage block. Understands the four wire
/// formats astria speaks — OpenAI-compatible (`usage.prompt_tokens` /
/// `completion_tokens`), Anthropic (`usage.input_tokens` / `output_tokens`),
/// Gemini (`usageMetadata.*TokenCount`), and AWS Bedrock Converse
/// (`usage.inputTokens` / `outputTokens`). A response without a usage
/// block still counts its call: an unmeasured call must never look free.
pub fn record_usage(response: &serde_json::Value) {
    USAGE_CALLS.fetch_add(1, Ordering::Relaxed);
    let usage = response
        .get("usage")
        .or_else(|| response.get("usageMetadata"));
    let Some(usage) = usage else {
        return;
    };
    let input = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .or_else(|| usage.get("promptTokenCount"))
        .or_else(|| usage.get("inputTokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .or_else(|| usage.get("candidatesTokenCount"))
        .or_else(|| usage.get("outputTokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    USAGE_INPUT.fetch_add(input, Ordering::Relaxed);
    USAGE_OUTPUT.fetch_add(output, Ordering::Relaxed);
}

/// Cap the run's total tokens (input + output). `0` disables the cap.
pub fn configure_budget(total_tokens: u64) {
    BUDGET.store(total_tokens, Ordering::Relaxed);
    RESERVED.store(0, Ordering::Relaxed);
}

/// Read `ASTRIA_LLM_BUDGET` (total tokens) once per process; 0 = unlimited.
pub fn budget_from_env() -> u64 {
    astria_core::env_var("LLM_BUDGET")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// True once the run's recorded spend has reached the configured budget.
pub fn budget_exceeded() -> bool {
    let budget = BUDGET.load(Ordering::Relaxed);
    budget > 0 && usage_snapshot().total() >= budget
}

/// Error for callers that must stop when the budget is gone.
pub fn ensure_budget() -> Result<()> {
    if budget_exceeded() {
        return Err(AstriaError::Graph(
            "LLM token budget exhausted (ASTRIA_LLM_BUDGET) — raising the cap or caching \
             more files will resume extraction"
                .into(),
        ));
    }
    Ok(())
}

/// Tokens claimed by in-flight requests. A check-then-call sequence lets
/// every concurrent worker pass the same check and collectively overshoot
/// the cap; the reservation is claimed atomically before a request flies
/// and released (by drop) once the call finishes.
static RESERVED: AtomicU64 = AtomicU64::new(0);

/// Output allowance reserved per EXTRACTION request, tokens. This matches
/// the `max_tokens`/`maxOutputTokens` the backends put on the wire for
/// extraction calls — the reserve must cover what a response can cost.
pub const MAX_OUTPUT_TOKENS_EXTRACT: u64 = 4096;
/// Output allowance for single-shot `complete()` calls (community naming,
/// deep linking, judge gate/rank). Matches the backends' complete() cap.
pub const MAX_OUTPUT_TOKENS_COMPLETE: u64 = 1024;

/// A claimed budget reservation. Released exactly once on drop — whether
/// the call succeeded, failed, or the worker unwound — so no path can
/// double-release or leak a claim.
#[derive(Debug)]
pub struct Reservation {
    estimate: u64,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.estimate > 0 {
            RESERVED.fetch_sub(
                self.estimate.min(RESERVED.load(Ordering::Acquire)),
                Ordering::AcqRel,
            );
        }
    }
}

/// Atomically claim a conservative allowance for one upcoming call:
/// roughly `input_chars / 4` input tokens plus the call's full output
/// allowance. The claim either fits under the remaining budget (recorded
/// spend + already-claimed reservations) or the call is refused before it
/// can cost anything. Every billable request — extraction chunks, vision
/// calls, `complete()` passes, judge requests — claims one. This bounds
/// spend, not bills it exactly: actual usage is still recorded per response
/// and the reservation is released when it lands — ASTRIA_LLM_BUDGET is
/// therefore an enforced estimate, not a metered invoice.
pub fn reserve_budget(input_chars: usize, output_allowance: u64) -> Result<Reservation> {
    let budget = BUDGET.load(Ordering::Relaxed);
    if budget == 0 {
        return Ok(Reservation { estimate: 0 });
    }
    let estimate = (input_chars as u64 / 4).saturating_add(output_allowance);
    loop {
        let spent = usage_snapshot().total();
        let reserved = RESERVED.load(Ordering::Acquire);
        if spent.saturating_add(reserved).saturating_add(estimate) > budget {
            return Err(AstriaError::Graph(
                "LLM token budget exhausted (ASTRIA_LLM_BUDGET) — raising the cap or caching \
                 more files will resume extraction"
                    .into(),
            ));
        }
        match RESERVED.compare_exchange_weak(
            reserved,
            reserved + estimate,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Ok(Reservation { estimate }),
            Err(_) => continue,
        }
    }
}

// ---------------------------------------------------------------------------
// Community naming
// ---------------------------------------------------------------------------

/// A community's thematic name and one-line description, produced by one
/// LLM call per community.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityNaming {
    pub label: String,
    pub summary: String,
}

/// Label length ceiling — thematic names must stay scannable in exports.
const MAX_LABEL_CHARS: usize = 60;
const MAX_SUMMARY_CHARS: usize = 200;
/// How many member symbols a naming prompt may carry.
pub const MAX_MEMBERS_PER_PROMPT: usize = 30;

pub fn community_label_system_prompt() -> &'static str {
    "You name communities of related code symbols for a knowledge graph. \
     Given a hub symbol and its member symbols, respond ONLY with valid JSON: \
     {\"label\": \"...\", \"summary\": \"...\"}. \
     label: a 2-5 word Title Case thematic name for the group's shared concern \
     (never copy a member symbol's name). \
     summary: one sentence, at most 140 characters, describing what these \
     symbols do together. If the symbols are too unrelated to name, respond \
     {\"label\": \"\", \"summary\": \"\"}."
}

pub fn community_label_user_prompt(hub_label: &str, size: usize, members: &[String]) -> String {
    let shown: Vec<String> = members
        .iter()
        .take(MAX_MEMBERS_PER_PROMPT)
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    format!(
        "Hub symbol: {hub_label}\nCommunity size: {size} symbols\nMembers: {}",
        shown.join(", ")
    )
}

/// One naming call. No chunking — the input is a symbol list, not a file.
pub fn summarize_community(
    backend: &dyn SemanticBackend,
    hub_label: &str,
    size: usize,
    members: &[String],
) -> Result<CommunityNaming> {
    let user_prompt = community_label_user_prompt(hub_label, size, members);
    let _reservation = reserve_budget(user_prompt.len(), MAX_OUTPUT_TOKENS_COMPLETE)?;
    let text = backend.complete(community_label_system_prompt(), &user_prompt)?;
    parse_community_naming(&text)
        .ok_or_else(|| AstriaError::Graph("community naming reply was not usable JSON".into()))
}

/// Parse a naming reply, tolerating surrounding prose like
/// `parse_extraction_text` does. An empty label means "unnameable" and is
/// treated as no result so the hub fallback stays.
pub fn parse_community_naming(text: &str) -> Option<CommunityNaming> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        label: String,
        #[serde(default)]
        summary: String,
    }
    let parse = |s: &str| -> Option<Raw> { serde_json::from_str(s).ok() };
    let raw = parse(trimmed).or_else(|| {
        let (start, end) = (trimmed.find('{')?, trimmed.rfind('}')?);
        parse(&trimmed[start..=end])
    })?;
    let label = raw.label.trim().to_string();
    if label.is_empty() {
        return None;
    }
    let label: String = label.chars().take(MAX_LABEL_CHARS).collect();
    let summary: String = raw.summary.trim().chars().take(MAX_SUMMARY_CHARS).collect();
    Some(CommunityNaming { label, summary })
}

// ---------------------------------------------------------------------------
// Deep concept linking
// ---------------------------------------------------------------------------

/// A cross-file concept link: one of the file's symbols related to one of
/// the graph's existing concept nodes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConceptLink {
    /// The file symbol's label, verbatim from the prompt's symbol list.
    pub symbol: String,
    /// The concept node's id, verbatim from the prompt's concept list.
    pub concept: String,
    pub relation: String,
}

/// Relations a concept link may carry; anything else is clamped.
pub const ALLOWED_LINK_RELATIONS: &[&str] = &["relates_to", "uses", "implements", "depends_on"];
/// Per-file ceiling — concept links are context, not an edge dump.
pub const MAX_LINKS_PER_FILE: usize = 15;
/// How many concepts a linking prompt may offer.
pub const MAX_CONCEPTS_PER_PROMPT: usize = 80;

pub fn deep_link_system_prompt() -> &'static str {
    "You link code symbols to existing knowledge-graph concept nodes. \
     Respond ONLY with valid JSON: \
     {\"links\": [{\"symbol\": \"...\", \"concept\": \"...\", \"relation\": \"...\"}]}. \
     symbol MUST be copied verbatim from the provided symbol list; \
     concept MUST be copied verbatim from the provided concept id list; \
     relation is one of: relates_to, uses, implements, depends_on. \
     Include only meaningful, specific links (at most 15 per file). \
     Return {\"links\": []} when nothing matches."
}

pub fn deep_link_user_prompt(
    file_display: &str,
    symbols: &[String],
    concepts: &[(String, String)],
) -> String {
    let symbol_list: Vec<String> = symbols.iter().map(|s| format!("- {}", s.trim())).collect();
    let concept_list: Vec<String> = concepts
        .iter()
        .take(MAX_CONCEPTS_PER_PROMPT)
        .map(|(id, label)| format!("- {}: {}", id, label))
        .collect();
    format!(
        "File: {file_display}\n\nSymbols in this file:\n{}\n\nKnown concepts (id: label):\n{}",
        symbol_list.join("\n"),
        concept_list.join("\n")
    )
}

/// One linking call per file. No chunking — the input is a symbol list.
pub fn link_concepts(
    backend: &dyn SemanticBackend,
    file_display: &str,
    symbols: &[String],
    concepts: &[(String, String)],
) -> Result<Vec<ConceptLink>> {
    let user_prompt = deep_link_user_prompt(file_display, symbols, concepts);
    let _reservation = reserve_budget(user_prompt.len(), MAX_OUTPUT_TOKENS_COMPLETE)?;
    let text = backend.complete(deep_link_system_prompt(), &user_prompt)?;
    Ok(parse_deep_links(&text))
}

/// Parse a linking reply, clamp relations to the schema, drop empty or
/// self-referential entries, and deduplicate (symbol, concept) pairs.
pub fn parse_deep_links(text: &str) -> Vec<ConceptLink> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    #[derive(serde::Deserialize)]
    struct RawLink {
        #[serde(default)]
        symbol: String,
        #[serde(default)]
        concept: String,
        #[serde(default)]
        relation: String,
    }
    #[derive(serde::Deserialize)]
    struct RawLinks {
        #[serde(default)]
        links: Vec<RawLink>,
    }
    let parse = |s: &str| -> Option<RawLinks> { serde_json::from_str(s).ok() };
    let raw = parse(trimmed).or_else(|| {
        let (start, end) = (trimmed.find('{')?, trimmed.rfind('}')?);
        parse(&trimmed[start..=end])
    });
    let Some(raw) = raw else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();
    for link in raw.links {
        let symbol = link.symbol.trim().to_string();
        let concept = link.concept.trim().to_string();
        if symbol.is_empty() || concept.is_empty() {
            continue;
        }
        let mut relation = link.relation.trim().to_string();
        if !ALLOWED_LINK_RELATIONS.contains(&relation.as_str()) {
            relation = "relates_to".to_string();
        }
        if seen.insert((symbol.clone(), concept.clone())) {
            links.push(ConceptLink {
                symbol,
                concept,
                relation,
            });
        }
        if links.len() >= MAX_LINKS_PER_FILE {
            break;
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SemanticExtraction;

    /// The usage counters are process-global; tests that assert on them
    /// must not interleave.
    static COUNTER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn obj(json: &str) -> serde_json::Value {
        serde_json::from_str(json).unwrap()
    }

    // -- Usage accounting --

    #[test]
    fn record_usage_understands_all_four_wire_formats() {
        let _guard = COUNTER_LOCK.lock().unwrap();
        reset_usage();
        record_usage(&obj(
            r#"{"usage": {"prompt_tokens": 100, "completion_tokens": 20}}"#,
        ));
        record_usage(&obj(
            r#"{"usage": {"input_tokens": 10, "output_tokens": 5}}"#,
        ));
        record_usage(&obj(
            r#"{"usageMetadata": {"promptTokenCount": 7, "candidatesTokenCount": 3}}"#,
        ));
        record_usage(&obj(
            r#"{"usage": {"inputTokens": 4, "outputTokens": 2, "totalTokens": 6}}"#,
        ));
        let usage = usage_snapshot();
        assert_eq!(usage.input, 121);
        assert_eq!(usage.output, 30);
        assert_eq!(usage.calls, 4);
    }

    #[test]
    fn missing_usage_block_still_counts_the_call() {
        let _guard = COUNTER_LOCK.lock().unwrap();
        reset_usage();
        record_usage(&obj(r#"{"choices": []}"#));
        let usage = usage_snapshot();
        assert_eq!(usage.calls, 1);
        assert_eq!(usage.total(), 0);
    }

    #[test]
    fn budget_blocks_when_spend_reaches_cap() {
        let _guard = COUNTER_LOCK.lock().unwrap();
        reset_usage();
        configure_budget(50);
        record_usage(&obj(
            r#"{"usage": {"prompt_tokens": 45, "completion_tokens": 5}}"#,
        ));
        assert!(budget_exceeded());
        assert!(ensure_budget().is_err());
        configure_budget(0);
        reset_usage();
    }

    #[test]
    fn budget_below_cap_passes() {
        let _guard = COUNTER_LOCK.lock().unwrap();
        reset_usage();
        configure_budget(500);
        record_usage(&obj(
            r#"{"usage": {"prompt_tokens": 45, "completion_tokens": 5}}"#,
        ));
        assert!(!budget_exceeded());
        configure_budget(0);
        reset_usage();
    }

    // -- Community naming --

    #[test]
    fn community_user_prompt_lists_hub_size_members() {
        let prompt = community_label_user_prompt(
            "login_handler()",
            12,
            &["login_handler()".into(), "session.rs".into()],
        );
        assert!(prompt.contains("Hub symbol: login_handler()"));
        assert!(prompt.contains("Community size: 12"));
        assert!(prompt.contains("session.rs"));
    }

    #[test]
    fn parse_community_naming_plain_and_prose_wrapped() {
        let plain = parse_community_naming(
            r#"{"label": "Authentication & Sessions", "summary": "Login and token handling."}"#,
        )
        .unwrap();
        assert_eq!(plain.label, "Authentication & Sessions");
        let prose = parse_community_naming(
            "Sure!\n{\"label\": \"Clustering\", \"summary\": \"Label propagation.\"}\nDone",
        )
        .unwrap();
        assert_eq!(prose.label, "Clustering");
    }

    #[test]
    fn unnameable_or_garbage_reply_is_none() {
        assert!(parse_community_naming("").is_none());
        assert!(parse_community_naming("no json").is_none());
        assert!(
            parse_community_naming(r#"{"label": "", "summary": "empty means unnameable"}"#)
                .is_none()
        );
    }

    #[test]
    fn oversized_naming_output_is_clamped() {
        let long_label = "X".repeat(500);
        let long_summary = "Y".repeat(500);
        let naming = parse_community_naming(&format!(
            r#"{{"label": "{long_label}", "summary": "{long_summary}"}}"#
        ))
        .unwrap();
        assert!(naming.label.chars().count() <= MAX_LABEL_CHARS);
        assert!(naming.summary.chars().count() <= MAX_SUMMARY_CHARS);
    }

    // -- Deep linking --

    #[test]
    fn parse_deep_links_clamps_and_dedupes() {
        let links = parse_deep_links(
            r#"{"links": [
                {"symbol": "login()", "concept": "auth", "relation": "uses"},
                {"symbol": "login()", "concept": "auth", "relation": "uses"},
                {"symbol": "db()", "concept": "storage", "relation": "forks"},
                {"symbol": "", "concept": "x", "relation": "uses"},
                {"symbol": "y", "concept": "", "relation": "uses"}
            ]}"#,
        );
        assert_eq!(links.len(), 2, "dupes and empty endpoints dropped");
        assert_eq!(links[0].relation, "uses");
        assert_eq!(links[1].relation, "relates_to", "unknown relation clamped");
    }

    #[test]
    fn deep_link_cap_enforced() {
        let links: Vec<String> = (0..40)
            .map(|i| {
                format!(r#"{{"symbol": "s{i}", "concept": "c{i}", "relation": "relates_to"}}"#)
            })
            .collect();
        let parsed = parse_deep_links(&format!(r#"{{"links": [{}]}}"#, links.join(",")));
        assert_eq!(parsed.len(), MAX_LINKS_PER_FILE);
    }

    #[test]
    fn garbage_link_reply_is_empty_not_error() {
        assert!(parse_deep_links("").is_empty());
        assert!(parse_deep_links("no json here").is_empty());
    }

    #[test]
    fn deep_link_prompt_contains_both_lists() {
        let prompt = deep_link_user_prompt(
            "src/auth.rs",
            &["login()".into()],
            &[("auth".into(), "Authentication".into())],
        );
        assert!(prompt.contains("File: src/auth.rs"));
        assert!(prompt.contains("- login()"));
        assert!(prompt.contains("- auth: Authentication"));
    }

    // -- complete() through a stub backend --

    struct StubBackend {
        reply: String,
    }

    impl SemanticBackend for StubBackend {
        fn extract_semantic(&self, _c: &str, _f: &str) -> Result<SemanticExtraction> {
            Ok(SemanticExtraction::empty())
        }
        fn complete(&self, _system: &str, _user: &str) -> Result<String> {
            Ok(self.reply.clone())
        }
    }

    #[test]
    fn summarize_community_via_complete_roundtrip() {
        let _guard = COUNTER_LOCK.lock().unwrap();
        let backend = StubBackend {
            reply: "{\"label\": \"Persistence\", \"summary\": \"SQLite storage.\"}".into(),
        };
        let naming =
            summarize_community(&backend, "run_pipeline()", 5, &["run_pipeline()".into()]).unwrap();
        assert_eq!(naming.label, "Persistence");
    }

    #[test]
    fn link_concepts_via_complete_roundtrip() {
        let _guard = COUNTER_LOCK.lock().unwrap();
        let backend = StubBackend {
            reply:
                "{\"links\": [{\"symbol\": \"a\", \"concept\": \"c1\", \"relation\": \"uses\"}]}"
                    .into(),
        };
        let links = link_concepts(
            &backend,
            "f.rs",
            &["a".into()],
            &[("c1".into(), "C".into())],
        )
        .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].concept, "c1");
    }

    #[test]
    fn budget_blocks_complete_calls_too() {
        let _guard = COUNTER_LOCK.lock().unwrap();
        configure_budget(1);
        reset_usage();
        record_usage(&obj(
            r#"{"usage": {"prompt_tokens": 10, "completion_tokens": 10}}"#,
        ));
        let backend = StubBackend { reply: "{}".into() };
        assert!(summarize_community(&backend, "h", 1, &[]).is_err());
        assert!(link_concepts(&backend, "f", &[], &[]).is_err());
        configure_budget(0);
        reset_usage();
    }
}
