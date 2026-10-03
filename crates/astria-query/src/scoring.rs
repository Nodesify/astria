//! Lexical retrieval scoring: tokenization, stemming, corpus mode, entry
//! intent, and `score_nodes`.
//!
//! Split from lib.rs; no behavior change.
#![allow(unused_imports)]

use super::*;
use astria_paths::relative_display;
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// Lowercase word tokens, splitting camelCase / snake_case / kebab-case and
/// punctuation so "parseExtraction", "parse_extraction" and
/// "parse-extraction" all tokenize identically. CJK runs (Chinese, Japanese,
/// Korean) carry no such boundaries — a whole sentence would otherwise come
/// back as one useless token — so they are segmented with jieba before
/// joining the token list. Non-CJK tokenization is unchanged.
pub(crate) fn tokenize(s: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            let prev_upper = current.chars().last().is_some_and(|c| c.is_uppercase());
            if !current.is_empty() && ch.is_uppercase() && !prev_upper {
                tokens.push(std::mem::take(&mut current).to_lowercase());
            }
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current).to_lowercase());
        }
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }
    if tokens.iter().any(|t| contains_cjk(t)) {
        tokens = tokens
            .into_iter()
            .flat_map(|t| {
                if contains_cjk(&t) {
                    segment_cjk(&t)
                } else {
                    vec![t]
                }
            })
            .collect();
    }
    tokens
}

/// True when `s` holds any CJK ideograph, kana, or hangul — scripts where
/// word boundaries are not written and lexical matching needs segmentation.
pub(crate) fn contains_cjk(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(c as u32,
            0x3040..=0x30FF       // Hiragana + Katakana
            | 0x3400..=0x4DBF     // CJK Extension A
            | 0x4E00..=0x9FFF     // CJK Unified Ideographs
            | 0xAC00..=0xD7AF     // Hangul syllables
            | 0xF900..=0xFAFF     // CJK Compatibility Ideographs
            | 0x20000..=0x2FA1F) // CJK Extensions B–F
    })
}

/// Segment one CJK-bearing token with jieba (dictionary + HMM for words the
/// dictionary misses). The segmenter is built once and shared; short-token
/// cuts are microseconds, and non-CJK text never reaches this path.
pub(crate) fn segment_cjk(token: &str) -> Vec<String> {
    static JIEBA: std::sync::OnceLock<jieba_rs::Jieba> = std::sync::OnceLock::new();
    let jieba = JIEBA.get_or_init(jieba_rs::Jieba::new);
    jieba
        .cut(token, true)
        .into_iter()
        .map(|word| word.to_lowercase())
        .filter(|word| !word.trim().is_empty())
        .collect()
}

/// The `k` node labels most similar to `query` — did-you-mean suggestions
/// so a failed lookup hands the agent something actionable instead of a
/// dead end. Best Jaro-Winkler score across the query's terms wins. Terms
/// come from the shared tokenizer so CJK queries segment instead of
/// arriving as one unsplittable run.
pub(crate) fn nearest_labels(loaded: &LoadedGraph, query: &str, k: usize) -> Vec<String> {
    let terms: Vec<String> = tokenize(query)
        .into_iter()
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .filter(|t| t.len() > 2)
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(f64, &String)> = Vec::new();
    for idx in loaded.graph.node_indices() {
        let label = &loaded.graph[idx].label;
        let label_lower = label.to_lowercase();
        let best = terms
            .iter()
            .map(|t| strsim::jaro_winkler(&label_lower, t))
            .fold(0.0_f64, f64::max);
        if best > 0.6 {
            scored.push((best, label));
        }
    }
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(b.1))
    });
    scored.truncate(k);
    scored.into_iter().map(|(_, l)| l.clone()).collect()
}

/// Strip a simple English plural suffix so "communities" matches
/// "community" and "users" matches "user". Cheap morphology for code terms.
/// Per-query scoring debug (`ASTRIA_QUERY_DEBUG_SCORES=1`): dumps the top
/// scored nodes with their match components plus the final seed list to
/// stderr, so a retrieval miss is diagnosable from one query run.
pub(crate) fn debug_scores_enabled() -> bool {
    std::env::var("ASTRIA_QUERY_DEBUG_SCORES")
        .ok()
        .is_some_and(|v| matches!(v.trim(), "1" | "true" | "on"))
}

pub(crate) fn truncate_label(s: &str) -> String {
    s.chars().take(34).collect()
}

pub(crate) fn stem(token: &str) -> &str {
    if token.len() > 4 && token.ends_with("ies") {
        &token[..token.len() - 3] // "communities" -> "communit" (matches "community" prefix-wise)
    } else if token.len() > 3 && token.ends_with('s') && !token.ends_with("ss") {
        &token[..token.len() - 1]
    } else {
        token
    }
}

/// Common filler and question words, filtered before scoring: they carry no
/// retrieval signal and let prose-heavy nodes win on stopwords alone.
pub(crate) const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "then", "than", "that", "this", "these", "those",
    "there", "here", "of", "in", "on", "at", "by", "for", "with", "from", "into", "about", "as",
    "is", "are", "was", "were", "be", "been", "being", "am", "do", "does", "did", "doing", "have",
    "has", "had", "having", "will", "would", "shall", "should", "can", "could", "may", "might",
    "must", "to", "too", "it", "its", "itself", "they", "them", "their", "we", "us", "our", "you",
    "your", "i", "me", "my", "he", "she", "his", "her", "him", "what", "which", "who", "whom",
    "whose", "when", "where", "why", "how", "not", "no", "also", "just", "very", "some", "any",
    "each", "other", "more", "most",
];

/// Import-graph degree per node: how many `imports` edges leave and enter
/// each node. Entry-point questions ("what is the CLI entry point") ask for
/// a structural fact — the file execution starts from — that the import DAG
/// encodes (imports many modules, imported by none) and no file name spells
/// out; lexical scoring can never see it.
pub(crate) fn import_degrees(
    loaded: &LoadedGraph,
) -> (
    std::collections::HashMap<NodeIndex, u32>,
    std::collections::HashMap<NodeIndex, u32>,
) {
    let (mut out, mut inc) = (
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    );
    for e in loaded.graph.edge_references() {
        if loaded.graph[e.id()].relation == "imports" {
            *out.entry(e.source()).or_insert(0u32) += 1;
            *inc.entry(e.target()).or_insert(0u32) += 1;
        }
    }
    (out, inc)
}

/// Test/spec/example files import many modules and are imported by none —
/// structurally indistinguishable from an entry file — but they are never
/// the program's front door, so they are excluded from entry candidacy.
pub(crate) fn is_testish_path(path: &str) -> bool {
    let p = path.to_lowercase().replace('\\', "/");
    p.split('/').any(|segment| {
        let words = tokenize(segment);
        words.iter().any(|word| {
            matches!(
                word.as_str(),
                "test"
                    | "tests"
                    | "spec"
                    | "specs"
                    | "example"
                    | "examples"
                    | "fixture"
                    | "fixtures"
                    | "benchmark"
                    | "benchmarks"
            )
        })
    })
}

/// Word sequences whose presence marks a question as asking for the
/// program's starting file rather than a concept. Matched as intact,
/// stem-equal token runs — never as substrings: "main file" must not fire
/// inside "domain files". Bare "bootstrap" is deliberately absent: too
/// many repos contain the CSS framework of that name, and a false entry
/// intent re-ranks import roots above every lexical match the question
/// actually earned.
pub(crate) const ENTRY_INTENT_PHRASES: &[&[&str]] = &[
    &["entry", "point"],
    &["entrypoint"],
    &["main", "file"],
    &["starting", "point"],
];

/// True when `tokens` contains an entry-intent phrase as a contiguous word
/// run, plural stems included ("entry points", "the entrypoints").
pub(crate) fn has_entry_intent(tokens: &[String]) -> bool {
    ENTRY_INTENT_PHRASES.iter().any(|phrase| {
        !phrase.is_empty()
            && tokens.windows(phrase.len()).any(|window| {
                window
                    .iter()
                    .zip(phrase.iter())
                    .all(|(token, word)| token.as_str() == *word || stem(token) == stem(word))
            })
    })
}

/// Minimum outgoing imports for a file to count as an entry candidate —
/// below this it is a leaf module, not a front door.
pub(crate) const ENTRY_MIN_IMPORTS: u32 = 3;

/// IDF floor/floor-cap: even a term in every label keeps a quarter of its
/// label weight, so ubiquitous terms still break ties, just never dominate.
pub(crate) const IDF_FLOOR: f64 = 0.25;

/// Score complete normalized identifiers above partial component matches.
pub(crate) fn normalized_identifier(text: &str) -> String {
    tokenize(text).concat()
}

/// True when `term` is a qualified name that matches the node id's token
/// tail or a scope token ("BaseCommand.get_usage" matches
/// `src_click_core_basecommand::get_usage` via its [basecommand, get,
/// usage] suffix; "BaseCommand" matches the basecommand scope token).
/// Same-name symbols share one bare label, so the qualified scope in the
/// id is the only lexical place the class lives — and the seed reservation
/// must honor it or the label tie hands the slot to a same-name stranger.
pub(crate) fn qualified_scope_match(term: &str, id: &str) -> bool {
    let want = normalized_identifier(term);
    if want.is_empty() {
        return false;
    }
    let tokens = tokenize(id);
    if tokens.contains(&want) {
        return true;
    }
    for start in 0..tokens.len() {
        if tokens[start..].concat() == want {
            return true;
        }
    }
    false
}

/// Reservation applies to written identifiers, not ordinary prose terms.
pub(crate) fn is_explicit_identifier(term: &str) -> bool {
    let quoted = term.starts_with(['`', '\"', '\'']);
    let text = term.trim_matches(|c: char| matches!(c, '`' | '\"' | '\'' | ',' | '?' | '!' | ';'));
    quoted
        || text.contains(['_', '-', '.', '/', '\\', ':', '('])
        || text
            .chars()
            .zip(text.chars().skip(1))
            .any(|(a, b)| a.is_lowercase() && b.is_uppercase())
}

pub(crate) fn component_coverage(needle: &[String], haystack: &[String]) -> f64 {
    if needle.is_empty() {
        return 0.0;
    }
    needle
        .iter()
        .filter(|part| {
            haystack
                .iter()
                .any(|word| word == *part || stem(word) == stem(part))
        })
        .count() as f64
        / needle.len() as f64
}

/// Prose node types: chunked document bodies share the document lifecycle
/// (seed quota, priors) with whole-document nodes.
pub(crate) fn is_doc_type(file_type: &str) -> bool {
    matches!(file_type, "document" | "reference" | "paper" | "chunk")
}

/// Node types written only by semantic (LLM) extraction — concepts, entities,
/// patterns, and modules derived from prose. Structural graphs never contain
/// them, so priors and quotas keyed on these types leave plain pipelines
/// untouched.
pub(crate) fn is_semantic_type(file_type: &str) -> bool {
    matches!(file_type, "concept" | "entity" | "pattern" | "module")
}

/// Share of prose-like nodes: documents plus semantic-derived summaries. On
/// an LLM-enriched docs-only corpus the concept/code nodes the extractor adds
/// would otherwise push the document share under `DOCS_MAJORITY_PROSE_SHARE`
/// and strand the corpus with two prose seeds.
pub(crate) fn prose_share(loaded: &LoadedGraph) -> f64 {
    let prose = loaded
        .graph
        .node_indices()
        .filter(|&i| {
            is_doc_type(&loaded.graph[i].file_type) || is_semantic_type(&loaded.graph[i].file_type)
        })
        .count();
    prose as f64 / loaded.graph.node_count().max(1) as f64
}

/// Which corpus a query runs against — the designed distinction that
/// decides whether prose nodes (documents, chunks) rank as first-class
/// content or under code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CorpusMode {
    /// Code-majority graph: code symbols rank first, doc seeds are capped,
    /// chunk bodies score under code.
    CodeMajority,
    /// Docs-majority graph (transcript corpora, docs-only sites): there is
    /// no code to protect, so prose nodes ARE the corpus — the doc-seed
    /// quota opens and chunks rank like documents.
    DocsMajority,
}

/// Prose share at and above which auto-detection calls a graph
/// docs-majority. A designed threshold, not a tuning knob: mixed graphs
/// (real repos — mostly code plus docs) stay code-majority; corpora that
/// are effectively all prose cross it. Pin either way with
/// `ASTRIA_CORPUS_MODE=docs|code` when auto-detection guesses wrong.
pub(crate) const DOCS_MAJORITY_PROSE_SHARE: f64 = 0.95;

/// Parsed `ASTRIA_CORPUS_MODE=docs|code` pin. Unrecognized values warn and
/// fall back to auto-detection — a typo must not silently pin a mode.
pub(crate) fn corpus_mode_pin() -> Option<CorpusMode> {
    let value = astria_core::env_var("CORPUS_MODE")?;
    match corpus_mode_pin_value(value.trim()) {
        Some(mode) => Some(mode),
        None => {
            eprintln!(
                "warning: ASTRIA_CORPUS_MODE={value:?} not recognized (docs|code); using auto-detection"
            );
            None
        }
    }
}

pub(crate) fn corpus_mode_pin_value(value: &str) -> Option<CorpusMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "docs" | "doc" | "documents" => Some(CorpusMode::DocsMajority),
        "code" => Some(CorpusMode::CodeMajority),
        _ => None,
    }
}

/// The corpus mode for one loaded graph: the env pin when set, otherwise
/// auto-detection from the prose share.
pub(crate) fn corpus_mode(loaded: &LoadedGraph) -> CorpusMode {
    corpus_mode_from(prose_share(loaded), corpus_mode_pin())
}

pub(crate) fn corpus_mode_from(share: f64, pin: Option<CorpusMode>) -> CorpusMode {
    pin.unwrap_or(if share >= DOCS_MAJORITY_PROSE_SHARE {
        CorpusMode::DocsMajority
    } else {
        CorpusMode::CodeMajority
    })
}

pub(crate) fn wants_docs(terms: &[String]) -> bool {
    terms.iter().flat_map(|t| tokenize(t)).any(|t| {
        matches!(
            t.as_str(),
            "docs" | "documentation" | "readme" | "guide" | "tutorial"
        )
    })
}

/// Ranked nodes plus the evidence the ranking was built on.
///
/// `salient_terms` are the query's highest-IDF terms — the ones that
/// identify the answer — and `max_salient_hits` is the best salient-term
/// coverage any node achieved. `max_matched_terms` of `effective_count`
/// is the best term coverage any node achieved at all, `entry_intent`
/// records whether structural entry-point candidates exist (those queries
/// are lexical-by-design). `missing_terms` are the effective terms that
/// matched nothing anywhere. Together these let callers tell "the graph
/// has evidence for what the question is about" from "nothing matched the
/// question's identifying terms" (the no-confident-match case).
pub(crate) struct ScoredNodes {
    pub(crate) ranked: Vec<(f64, NodeIndex)>,
    pub(crate) salient_terms: Vec<String>,
    pub(crate) max_salient_hits: usize,
    pub(crate) max_matched_terms: usize,
    pub(crate) effective_count: usize,
    pub(crate) entry_intent: bool,
    pub(crate) missing_terms: Vec<String>,
}

pub(crate) fn score_nodes(loaded: &LoadedGraph, terms: &[String]) -> ScoredNodes {
    // IDF weights + per-node lowercase labels, one shared pre-pass.
    let n_nodes = loaded.graph.node_count().max(1) as f64;
    let ln_nodes = n_nodes.ln().max(1.0);
    let labels_lower: Vec<String> = loaded
        .graph
        .node_indices()
        .map(|idx| loaded.graph[idx].label.to_lowercase())
        .collect();
    let doc_share = prose_share(loaded);
    let docs_majority = doc_share >= DOCS_MAJORITY_PROSE_SHARE;
    let label_components: Vec<Vec<String>> = loaded
        .graph
        .node_indices()
        .map(|idx| tokenize(&loaded.graph[idx].label))
        .collect();
    // Chunk and document bodies feed the IDF pre-pass too: scoring matches
    // those terms against docstrings, so a term that is common in bodies
    // but rare in first lines ("group", "friends" in transcripts) would
    // otherwise get near-max weight and let any node that merely mentions
    // it outrank the node whose text actually answers.
    let doc_components: Vec<Vec<String>> = loaded
        .graph
        .node_indices()
        .map(|idx| {
            loaded.graph[idx]
                .docstring
                .as_deref()
                .map(|d| tokenize(d).into_iter().take(400).collect())
                .unwrap_or_default()
        })
        .collect();
    // Qualified ids participate in IDF like labels and bodies: id tokens
    // repeat across a repo ("src", "core", "tests") and must not act as
    // rare discriminators.
    let id_components: Vec<Vec<String>> = loaded
        .graph
        .node_indices()
        .map(|idx| tokenize(&loaded.graph[idx].id))
        .collect();
    let mut idf: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();
    let mut effective: Vec<&String> = Vec::new();
    for term in terms {
        let t = term.trim();
        if t.len() <= 2 || STOPWORDS.contains(&t.to_lowercase().as_str()) {
            continue;
        }
        effective.push(term);
    }
    // Effective terms with zero coverage hits anywhere — the question's
    // vocabulary the graph does not share, named in a refusal message.
    let mut missing_terms: Vec<String> = Vec::new();
    for term in &effective {
        let parts = tokenize(term);
        let hits = label_components
            .iter()
            .zip(doc_components.iter().zip(id_components.iter()))
            .filter(|(label, (doc, id))| {
                component_coverage(&parts, label) == 1.0
                    || component_coverage(&parts, doc) == 1.0
                    || component_coverage(&parts, id) == 1.0
            })
            .count();
        let w = if hits == 0 {
            missing_terms.push((*term).clone());
            1.0
        } else {
            ((n_nodes / hits as f64).ln() / ln_nodes).clamp(IDF_FLOOR, 1.0)
        };
        idf.insert(term.as_str(), w);
    }
    let idf_weight = |term: &str| -> f64 { idf.get(term).copied().unwrap_or(1.0) };

    // Salient-term coverage: the query's highest-IDF terms are the ones
    // that identify the answer. A long natural-language description
    // (RepoQA-style numbered specifications, issue bodies) otherwise lets
    // nodes that match many WEAK terms ("line", "code", "output") outrank
    // the node matching the few strong ones — the unnormalized term sum
    // buried the true function outside the seed set. Nodes are scaled by
    // how much of the salient set they touch: full salient coverage keeps
    // the score, zero salient evidence is damped to 60%.
    let mut salient_terms: Vec<&String> = effective.clone();
    salient_terms.sort_by(|a, b| {
        idf_weight(b)
            .partial_cmp(&idf_weight(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(b))
    });
    let salient_k = effective.len().div_ceil(4).clamp(1, 3);
    salient_terms.truncate(salient_k);
    let salient_set: std::collections::HashSet<&str> =
        salient_terms.iter().map(|t| t.as_str()).collect();
    let salient_owned: Vec<String> = salient_terms.iter().map(|t| (*t).clone()).collect();

    // Entry-point intent: detect once, pay for the degree maps only then.
    // Phrase runs are matched over the token stream, not a joined string.
    let question_tokens: Vec<String> = terms.iter().flat_map(|t| tokenize(t)).collect();
    let wants_tests = question_tokens.iter().any(|t| {
        matches!(
            t.as_str(),
            "test"
                | "tests"
                | "testing"
                | "spec"
                | "specs"
                | "benchmark"
                | "benchmarks"
                | "example"
                | "examples"
        )
    });
    let wants_docs = wants_docs(terms);
    let wants_entry = has_entry_intent(&question_tokens);
    let (imports_out, imports_in) = if wants_entry {
        import_degrees(loaded)
    } else {
        (
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
        )
    };

    let mut scored: Vec<(f64, NodeIndex)> = Vec::new();
    let mut entry_candidates = HashSet::new();
    let debug_scores = debug_scores_enabled();
    // Per-node (matched_terms, salient_hits) in node-index order, for the dump.
    let mut debug_rows: Vec<(usize, usize)> = Vec::new();
    let mut max_salient_hits = 0usize;
    let mut max_matched_terms = 0usize;
    let effective_terms_count = terms
        .iter()
        .filter(|t| {
            let t = t.trim();
            t.len() > 2 && !STOPWORDS.contains(&t.to_lowercase().as_str())
        })
        .count();
    for (i, idx) in loaded.graph.node_indices().enumerate() {
        let node = &loaded.graph[idx];
        // Code answers rank above documentation and speculative stubs on
        // equal term evidence: without the prior, prose-heavy doc nodes and
        // std-call stubs crowd code symbols out of the seed set.
        let is_chunk = node.file_type == "chunk";
        let is_doc = is_chunk || is_doc_type(&node.file_type);
        let symbol_parts = &label_components[i];
        let is_test = node.file_type == "test"
            || is_testish_path(&node.source_file)
            || symbol_parts
                .first()
                .is_some_and(|p| matches!(p.as_str(), "test" | "tests" | "spec" | "bench"))
            || node.id.contains("::tests::");
        let prior = if is_test {
            if wants_tests {
                1.25
            } else {
                0.4
            }
        } else if is_chunk {
            // Chunked prose out-scoring code on body-term luck displaced
            // exact code answers by a couple of ranks; in code-majority
            // graphs chunks rank under documents, while on docs-majority
            // graphs they ARE the corpus and rank like any document.
            if docs_majority {
                if wants_docs {
                    1.25
                } else {
                    0.55
                }
            } else {
                0.45
            }
        } else if is_doc {
            if wants_docs {
                1.25
            } else {
                0.55
            }
        } else if is_semantic_type(&node.file_type) {
            // LLM-derived concept/summary nodes keyword-match prose questions
            // as strongly as the documents they were extracted from, and at
            // the code prior (1.0) they crowd both code symbols and the
            // primary document that actually holds the answer. Rank them
            // just under primary documents on doc-intent questions, under
            // code otherwise.
            if wants_docs {
                1.1
            } else {
                0.55
            }
        } else if node.file_type == "stub" {
            0.4
        } else {
            1.0
        };
        // Consecutive question tokens that appear verbatim in a node's label
        // or docstring ("blast radius") mark the node as the concept's home;
        // token-level scoring alone treats the words as unrelated and loses
        // to weaker-but-lexically-luckier matches.
        let doc_lower = node.docstring.as_deref().map(|d| d.to_lowercase());
        let label_lower_full = labels_lower[i].clone();
        let mut phrase_bonus = 0.0f64;
        for w in terms.windows(2) {
            let phrase = format!("{} {}", w[0].to_lowercase(), w[1].to_lowercase());
            let hit = label_lower_full.contains(&phrase)
                || doc_lower.as_deref().is_some_and(|d| d.contains(&phrase));
            if hit {
                phrase_bonus += 0.5;
            }
        }
        let phrase_bonus = phrase_bonus.min(1.0);
        let label_tokens = &label_components[i];
        let file_tokens = tokenize(&node.source_file);
        // Chunked prose bodies live in the docstring; 400 tokens keeps
        // whole-chunk scoring affordable while matching deep into the chunk.
        let doc_tokens: Vec<String> = node
            .docstring
            .as_deref()
            .map(|d| tokenize(d).into_iter().take(400).collect())
            .unwrap_or_default();
        let mut score = 0.0;
        let mut matched_terms = 0usize;
        let mut salient_hits = 0usize;
        for term in terms {
            let term = term.trim();
            if term.len() <= 2 {
                continue;
            }
            let term_lower = term.to_lowercase();
            // Question words carry no retrieval signal: "where is the SSRF
            // validation and what does it check" must score on ssrf/url/
            // validation/check, not on "where"/"does" matching every doc
            // heading that contains them.
            if STOPWORDS.contains(&term_lower.as_str()) {
                continue;
            }
            let term_tokens = tokenize(term);
            let normalized = normalized_identifier(term);
            let label_normalized = normalized_identifier(&node.label);
            let coverage = component_coverage(&term_tokens, label_tokens);
            // Chunk labels are a truncated first line of the chunk's own
            // body, which the docstring below scores in full. Amplifying
            // that prefix at label weight double-counts body text, so a
            // lucky opening line (a later session re-mentioning a topic)
            // outranks the chunk whose body actually answers the question.
            let label_score = if is_chunk {
                0.0
            } else if normalized == label_normalized {
                4.0
            } else if coverage == 1.0 {
                2.0
            } else {
                0.5 * coverage * coverage
            };
            let doc_coverage = component_coverage(&term_tokens, &doc_tokens);
            let path_coverage = component_coverage(&term_tokens, &file_tokens);
            // A qualified question term ("BaseCommand.get_usage") matches a
            // node's scope-qualified id even though every same-name symbol
            // shares one bare label ("get_usage()"); without id evidence the
            // tie falls to degree and a same-name symbol from the wrong
            // class wins the answer slot.
            let id_coverage = component_coverage(&term_tokens, &id_components[i]);
            let id_score = id_coverage * id_coverage;
            // A chunk's body is its content: body evidence there scores at
            // label parity, so label luck (a speaker name in the first line)
            // cannot outrank the chunk that actually answers the question.
            let doc_coeff = if is_chunk { 1.1 } else { 0.35 };
            let doc_score = doc_coeff * doc_coverage * doc_coverage;
            let path_score = 0.55 * path_coverage * path_coverage;
            let fuzzy_score = if label_score + doc_score + path_score == 0.0
                && term_tokens.len() == 1
                && label_tokens
                    .iter()
                    .any(|lt| strsim::jaro_winkler(lt, &term_tokens[0]) > 0.9)
            {
                0.15
            } else {
                0.0
            };
            if label_score + doc_score + path_score + id_score + fuzzy_score > 0.0 {
                matched_terms += 1;
                if salient_set.contains(term) {
                    salient_hits += 1;
                }
            }
            score += (label_score.max(doc_score) + id_score + path_score + fuzzy_score)
                * idf_weight(term);
        }
        // Questions are multi-term: a node covering most of them outranks a
        // lexically lucky single-term match ("paint" in a speaker line vs
        // the chunk holding melanie + painted + sunrise).
        if effective_terms_count > 0 {
            score *= 0.8 + 0.2 * (matched_terms as f64 / effective_terms_count as f64);
        }
        // Salient-term coverage scaling (see the salient_terms pre-pass):
        // matches on the query's rarest terms are worth structurally more
        // than matches on its common ones.
        if !salient_terms.is_empty() {
            score *= 0.6 + 0.4 * (salient_hits as f64 / salient_terms.len() as f64);
        }
        // Entry-point intent: a file that imports many modules and is
        // imported by none is the program's front door, whatever it is
        // named ("index.ts", "main.rs", "cli.py").
        if wants_entry
            && label_is_file(&node.label)
            && node.file_type == "code"
            && !is_test
            && imports_in.get(&idx).copied().unwrap_or(0) == 0
            && imports_out.get(&idx).copied().unwrap_or(0) >= ENTRY_MIN_IMPORTS
        {
            entry_candidates.insert(idx);
        }
        if score > 0.0 || entry_candidates.contains(&idx) {
            scored.push(((score + phrase_bonus) * prior, idx));
        }
        max_salient_hits = max_salient_hits.max(salient_hits);
        max_matched_terms = max_matched_terms.max(matched_terms);
        if debug_scores {
            debug_rows.push((matched_terms, salient_hits));
        }
    }
    // Explicit entry intent gives import roots precedence over lexical
    // mentions of "entry point". The tier is derived from this candidate
    // set, so changing lexical weights cannot drown out structural evidence.
    if !entry_candidates.is_empty() {
        let lexical_ceiling = scored
            .iter()
            .map(|(score, _)| *score)
            .fold(0.0_f64, f64::max);
        for (score, idx) in &mut scored {
            if entry_candidates.contains(idx) {
                *score += lexical_ceiling + 1.0;
            }
        }
    }
    // Deterministic order: score desc, then label, then id.
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| loaded.graph[a.1].label.cmp(&loaded.graph[b.1].label))
            .then_with(|| loaded.graph[a.1].id.cmp(&loaded.graph[b.1].id))
    });
    if debug_scores {
        eprintln!(
            "-- score dump: top {} of {} scored (matched/effective, salient/k) --",
            scored.len().min(15),
            scored.len()
        );
        for (rank, (score, idx)) in scored.iter().take(15).enumerate() {
            let n = &loaded.graph[*idx];
            let (matched, salient) = debug_rows.get(idx.index()).copied().unwrap_or((0, 0));
            eprintln!(
                "  #{:<2} score={:<9.3} matched={:<3}/{} salient={:<2}/{} {} [{}]",
                rank + 1,
                score,
                matched,
                effective_terms_count,
                salient,
                salient_terms.len(),
                truncate_label(&n.label),
                truncate_label(&n.id),
            );
        }
    }
    ScoredNodes {
        ranked: scored,
        salient_terms: salient_owned,
        max_salient_hits,
        max_matched_terms,
        effective_count: effective_terms_count,
        entry_intent: !entry_candidates.is_empty(),
        missing_terms,
    }
}
