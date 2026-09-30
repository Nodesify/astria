use std::collections::{HashMap, HashSet};

use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rusqlite::Connection;

use astria_paths::relative_display;

/// How many near-miss labels to suggest when a query matches nothing.
const SUGGESTION_COUNT: usize = 3;

fn log_query(db: &Connection, question: &str, answer: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let _ = db.execute(
        "INSERT INTO query_history (question, answer, path_taken, queried_at) VALUES (?1, ?2, '', ?3)",
        rusqlite::params![question, answer.chars().take(500).collect::<String>(), ts],
    );
}

#[derive(Debug)]
struct NodeData {
    id: String,
    label: String,
    file_type: String,
    source_file: String,
    source_line: Option<i64>,
    community: Option<i64>,
    docstring: Option<String>,
    signature: Option<String>,
}

#[derive(Debug)]
struct EdgeData {
    relation: String,
    confidence: String,
    confidence_score: Option<f64>,
    source_file: String,
    source_line: Option<i64>,
}

impl EdgeData {
    fn meets_detail(&self, min_strength: f64) -> bool {
        if min_strength >= 0.9 {
            matches!(
                self.confidence.to_ascii_uppercase().as_str(),
                "EXTRACTED" | "DECLARED"
            )
        } else {
            self.strength() >= min_strength
        }
    }

    /// Effective strength of this edge: the stored numeric score when
    /// present, otherwise a rank derived from the confidence label.
    fn strength(&self) -> f64 {
        self.confidence_score
            .unwrap_or_else(|| confidence_rank(&self.confidence))
    }
}

/// Nodes whose strongest touching edge falls below this floor are weakly
/// reached (only via low-confidence SEMANTIC links) and sort after all
/// strongly-reached nodes in query output. Label ranks for structural
/// edges start at 0.7 (INFERRED), so plain graphs never produce weak nodes.
const SEMANTIC_WEAK_FLOOR: f64 = 0.65;

/// Minimum rescaled embedding score for the semantic-only seed reservation:
/// the measured slot floor from `astria_core::calibration` (raw cosine 0.52
/// on the shipped model) — strong enough that the candidate is genuinely
/// about the question, not a distant neighbor.
const SEMANTIC_SEED_FLOOR: f64 =
    astria_core::calibration::SEMANTIC_CALIBRATION.seed_slot_score_floor();

/// Fallback strength for edges without a numeric score. Alphabetical
/// string comparison of confidence labels does NOT order by strength
/// ("SEMANTIC" > "LLM" lexicographically), so map labels to numbers.
fn confidence_rank(confidence: &str) -> f64 {
    match confidence.to_uppercase().as_str() {
        "DECLARED" => 1.0,
        "EXTRACTED" => 0.9,
        // A call expression extracted from source whose bare name bound to
        // exactly one definition: stronger than co-occurrence inference,
        // deliberately below the EXTRACTED/DECLARED tier so `--detail high`
        // (compiler-grade facts) still excludes it.
        "RESOLVED" => 0.85,
        "INFERRED" => 0.7,
        "SEMANTIC" => 0.6,
        _ => 0.5,
    }
}

struct LoadedGraph {
    /// Directed storage even though most traversals are undirected: the
    /// edge orientation (caller → callee, importer → module) is preserved,
    /// and `directed` queries can follow it.
    graph: DiGraph<NodeData, EdgeData>,
    id_to_idx: HashMap<String, NodeIndex>,
    /// Project root derived from the DB path (`root/.astria/db.sqlite`),
    /// used to shorten stored absolute paths in agent-facing output.
    root: Option<String>,
}

impl LoadedGraph {
    /// Root-relative display form of a stored path.
    fn display_path(&self, path: &str) -> String {
        match &self.root {
            Some(root) => relative_display(path, root),
            None => path.trim_start_matches("//?/").to_string(),
        }
    }
}

fn load_graph(db: &Connection, db_path: &str) -> astria_core::Result<LoadedGraph> {
    // Project root: two levels above the DB file (root/.astria/db.sqlite).
    let root = std::path::Path::new(db_path)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_string_lossy().replace('\\', "/"));

    let mut nodes = Vec::new();
    {
        let mut stmt = db.prepare(
            "SELECT id, label, file_type, source_file, source_line, community, docstring, signature FROM nodes",
        )?;
        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            String,
            String,
            String,
            String,
            Option<i64>,
            Option<i64>,
            Option<String>,
            Option<String>,
        )> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        for (id, label, ft, sf, line, comm, doc, sig) in rows {
            nodes.push((id, label, ft, sf, line, comm, doc, sig));
        }
    }

    let mut graph = DiGraph::new();
    let mut id_to_idx = HashMap::new();
    for (id, label, ft, sf, line, comm, doc, sig) in &nodes {
        let idx = graph.add_node(NodeData {
            id: id.clone(),
            label: label.clone(),
            file_type: ft.clone(),
            source_file: sf.clone(),
            source_line: *line,
            community: *comm,
            docstring: doc.clone(),
            signature: sig.clone(),
        });
        id_to_idx.insert(id.clone(), idx);
    }

    {
        let mut stmt = db.prepare(
            "SELECT source, target, relation, confidence, confidence_score, source_file, source_line FROM edges",
        )?;
        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            String,
            String,
            String,
            String,
            Option<f64>,
            String,
            Option<i64>,
        )> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        for (src, tgt, rel, conf, score, sf, line) in rows {
            if let (Some(&s), Some(&t)) = (id_to_idx.get(&src), id_to_idx.get(&tgt)) {
                graph.add_edge(
                    s,
                    t,
                    EdgeData {
                        relation: rel,
                        confidence: conf,
                        confidence_score: score,
                        source_file: sf,
                        source_line: line,
                    },
                );
            }
        }
    }

    Ok(LoadedGraph {
        graph,
        id_to_idx,
        root,
    })
}

// Read both tables in one SQLite snapshot. Reloading avoids stale state after
// external commits, local writes, and rollbacks.
fn read_snapshot(db: &Connection) -> astria_core::Result<Option<rusqlite::Transaction<'_>>> {
    Ok(if db.is_autocommit() {
        Some(db.unchecked_transaction()?)
    } else {
        None
    })
}

fn load_graph_snapshot(db: &Connection, db_path: &str) -> astria_core::Result<LoadedGraph> {
    let transaction = read_snapshot(db)?;
    let loaded = load_graph(db, db_path)?;
    if let Some(transaction) = transaction {
        transaction.commit()?;
    }
    Ok(loaded)
}

/// Neighbors of `idx`: outgoing only when traversing a directed graph,
/// both directions otherwise.
fn iter_neighbors<'a>(
    graph: &'a DiGraph<NodeData, EdgeData>,
    idx: NodeIndex,
    directed: bool,
) -> impl Iterator<Item = NodeIndex> + 'a {
    let outgoing = graph.neighbors_directed(idx, Direction::Outgoing);
    if directed {
        Box::new(outgoing) as Box<dyn Iterator<Item = NodeIndex> + 'a>
    } else {
        Box::new(outgoing.chain(graph.neighbors_directed(idx, Direction::Incoming)))
            as Box<dyn Iterator<Item = NodeIndex> + 'a>
    }
}

/// Like `iter_neighbors`, but only crossing edges whose confidence strength
/// meets `min_strength` — the fidelity-tier filter (`--detail high` keeps
/// only EXTRACTED/DECLARED facts and drops INFERRED/SEMANTIC ones).
fn iter_neighbors_filtered<'a>(
    graph: &'a DiGraph<NodeData, EdgeData>,
    idx: NodeIndex,
    directed: bool,
    min_strength: f64,
    semantic_floor: f64,
) -> impl Iterator<Item = (NodeIndex, EdgeIndex)> + 'a {
    graph
        .edges_directed(idx, Direction::Outgoing)
        .filter(move |e| {
            e.weight().meets_detail(min_strength)
                && !below_semantic_floor(e.weight(), semantic_floor)
        })
        .map(|e| (e.target(), e.id()))
        .chain(
            graph
                .edges_directed(idx, Direction::Incoming)
                .filter(move |e| {
                    !directed
                        && e.weight().meets_detail(min_strength)
                        && !below_semantic_floor(e.weight(), semantic_floor)
                })
                .map(|e| (e.source(), e.id())),
        )
}

/// Opt-in hard floor (`ASTRIA_QUERY_MIN_SEMANTIC_CONFIDENCE`, 0.0 = off):
/// SEMANTIC edges whose calibrated keep-probability falls below the floor
/// are excluded from traversal entirely. Structural and inferred edges are
/// never touched — this is how graph consumers act on the judge's
/// existence verdicts at query time.
fn below_semantic_floor(edge: &EdgeData, floor: f64) -> bool {
    floor > 0.0 && edge.confidence.eq_ignore_ascii_case("SEMANTIC") && edge.strength() < floor
}

/// The strongest edge connecting `a` and `b`, in either direction.
fn edge_between(
    graph: &DiGraph<NodeData, EdgeData>,
    a: NodeIndex,
    b: NodeIndex,
) -> Option<&EdgeData> {
    let forward = graph
        .edges_directed(a, Direction::Outgoing)
        .find(|e| e.target() == b)
        .map(|e| e.weight());
    match forward {
        Some(w) => Some(w),
        None => graph
            .edges_directed(b, Direction::Outgoing)
            .find(|e| e.target() == a)
            .map(|e| e.weight()),
    }
}

/// Lowercase word tokens, splitting camelCase / snake_case / kebab-case and
/// punctuation so "parseExtraction", "parse_extraction" and
/// "parse-extraction" all tokenize identically.
fn tokenize(s: &str) -> Vec<String> {
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
    tokens
}

/// The `k` node labels most similar to `query` — did-you-mean suggestions
/// so a failed lookup hands the agent something actionable instead of a
/// dead end. Best Jaro-Winkler score across the query's terms wins.
fn nearest_labels(loaded: &LoadedGraph, query: &str, k: usize) -> Vec<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .filter(|t| t.len() > 2)
        .map(|t| t.to_lowercase())
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
fn debug_scores_enabled() -> bool {
    std::env::var("ASTRIA_QUERY_DEBUG_SCORES")
        .ok()
        .is_some_and(|v| matches!(v.trim(), "1" | "true" | "on"))
}

fn truncate_label(s: &str) -> String {
    s.chars().take(34).collect()
}

fn stem(token: &str) -> &str {
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
const STOPWORDS: &[&str] = &[
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
fn import_degrees(
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
fn is_testish_path(path: &str) -> bool {
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
const ENTRY_INTENT_PHRASES: &[&[&str]] = &[
    &["entry", "point"],
    &["entrypoint"],
    &["main", "file"],
    &["starting", "point"],
];

/// True when `tokens` contains an entry-intent phrase as a contiguous word
/// run, plural stems included ("entry points", "the entrypoints").
fn has_entry_intent(tokens: &[String]) -> bool {
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
const ENTRY_MIN_IMPORTS: u32 = 3;

/// IDF floor/floor-cap: even a term in every label keeps a quarter of its
/// label weight, so ubiquitous terms still break ties, just never dominate.
const IDF_FLOOR: f64 = 0.25;

/// Score complete normalized identifiers above partial component matches.
fn normalized_identifier(text: &str) -> String {
    tokenize(text).concat()
}

/// True when `term` is a qualified name that matches the node id's token
/// tail or a scope token ("BaseCommand.get_usage" matches
/// `src_click_core_basecommand::get_usage` via its [basecommand, get,
/// usage] suffix; "BaseCommand" matches the basecommand scope token).
/// Same-name symbols share one bare label, so the qualified scope in the
/// id is the only lexical place the class lives — and the seed reservation
/// must honor it or the label tie hands the slot to a same-name stranger.
fn qualified_scope_match(term: &str, id: &str) -> bool {
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
fn is_explicit_identifier(term: &str) -> bool {
    let quoted = term.starts_with(['`', '\"', '\'']);
    let text = term.trim_matches(|c: char| matches!(c, '`' | '\"' | '\'' | ',' | '?' | '!' | ';'));
    quoted
        || text.contains(['_', '-', '.', '/', '\\', ':', '('])
        || text
            .chars()
            .zip(text.chars().skip(1))
            .any(|(a, b)| a.is_lowercase() && b.is_uppercase())
}

fn component_coverage(needle: &[String], haystack: &[String]) -> f64 {
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
fn is_doc_type(file_type: &str) -> bool {
    matches!(file_type, "document" | "reference" | "paper" | "chunk")
}

/// Node types written only by semantic (LLM) extraction — concepts, entities,
/// patterns, and modules derived from prose. Structural graphs never contain
/// them, so priors and quotas keyed on these types leave plain pipelines
/// untouched.
fn is_semantic_type(file_type: &str) -> bool {
    matches!(file_type, "concept" | "entity" | "pattern" | "module")
}

/// Share of prose-like nodes: documents plus semantic-derived summaries. On
/// an LLM-enriched docs-only corpus the concept/code nodes the extractor adds
/// would otherwise push the document share under `DOCS_MAJORITY_PROSE_SHARE`
/// and strand the corpus with two prose seeds.
fn prose_share(loaded: &LoadedGraph) -> f64 {
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
enum CorpusMode {
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
const DOCS_MAJORITY_PROSE_SHARE: f64 = 0.95;

/// Parsed `ASTRIA_CORPUS_MODE=docs|code` pin. Unrecognized values warn and
/// fall back to auto-detection — a typo must not silently pin a mode.
fn corpus_mode_pin() -> Option<CorpusMode> {
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

fn corpus_mode_pin_value(value: &str) -> Option<CorpusMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "docs" | "doc" | "documents" => Some(CorpusMode::DocsMajority),
        "code" => Some(CorpusMode::CodeMajority),
        _ => None,
    }
}

/// The corpus mode for one loaded graph: the env pin when set, otherwise
/// auto-detection from the prose share.
fn corpus_mode(loaded: &LoadedGraph) -> CorpusMode {
    corpus_mode_from(prose_share(loaded), corpus_mode_pin())
}

fn corpus_mode_from(share: f64, pin: Option<CorpusMode>) -> CorpusMode {
    pin.unwrap_or(if share >= DOCS_MAJORITY_PROSE_SHARE {
        CorpusMode::DocsMajority
    } else {
        CorpusMode::CodeMajority
    })
}

fn wants_docs(terms: &[String]) -> bool {
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
struct ScoredNodes {
    ranked: Vec<(f64, NodeIndex)>,
    salient_terms: Vec<String>,
    max_salient_hits: usize,
    max_matched_terms: usize,
    effective_count: usize,
    entry_intent: bool,
    missing_terms: Vec<String>,
}

fn score_nodes(loaded: &LoadedGraph, terms: &[String]) -> ScoredNodes {
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

/// `(visited nodes, observed edges, hop distance from the seeds)`.
type TraversalResult = (HashSet<NodeIndex>, Vec<EdgeIndex>, HashMap<NodeIndex, u32>);

fn bfs_subgraph(
    loaded: &LoadedGraph,
    start_nodes: &[NodeIndex],
    max_depth: usize,
    directed: bool,
    min_strength: f64,
    semantic_floor: f64,
) -> TraversalResult {
    let mut visited: HashSet<NodeIndex> = start_nodes.iter().copied().collect();
    let mut frontier: Vec<NodeIndex> = start_nodes.to_vec();
    let mut edges_seen: Vec<EdgeIndex> = Vec::new();
    let mut distance: HashMap<NodeIndex, u32> = start_nodes.iter().map(|&n| (n, 0)).collect();

    for depth in 0..max_depth {
        let mut next_frontier = Vec::new();
        for &node in &frontier {
            for (neighbor, edge_id) in
                iter_neighbors_filtered(&loaded.graph, node, directed, min_strength, semantic_floor)
            {
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    distance.insert(neighbor, depth as u32 + 1);
                    next_frontier.push(neighbor);
                    edges_seen.push(edge_id);
                }
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }
    (visited, edges_seen, distance)
}

fn dfs_subgraph(
    loaded: &LoadedGraph,
    start_nodes: &[NodeIndex],
    max_depth: usize,
    directed: bool,
    min_strength: f64,
    semantic_floor: f64,
) -> TraversalResult {
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut edges_seen: Vec<EdgeIndex> = Vec::new();
    let mut stack: Vec<(NodeIndex, usize)> = start_nodes.iter().rev().map(|&n| (n, 0)).collect();

    while let Some((node, depth)) = stack.pop() {
        if visited.contains(&node) || depth > max_depth {
            continue;
        }
        visited.insert(node);
        if depth == max_depth {
            continue;
        }
        for (neighbor, edge_id) in
            iter_neighbors_filtered(&loaded.graph, node, directed, min_strength, semantic_floor)
        {
            if !visited.contains(&neighbor) {
                stack.push((neighbor, depth + 1));
                edges_seen.push(edge_id);
            }
        }
    }
    (visited, edges_seen, HashMap::new())
}

/// A label that names a file ("lib.rs", "benchmark.md") rather than a symbol.
fn label_is_file(label: &str) -> bool {
    match label.rfind('.') {
        Some(dot) if dot > 0 => {
            let ext = &label[dot + 1..];
            !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn subgraph_to_text(
    loaded: &LoadedGraph,
    visited: &HashSet<NodeIndex>,
    edges_seen: &[EdgeIndex],
    relevance: &HashMap<NodeIndex, f64>,
    distance: &HashMap<NodeIndex, u32>,
    reach_strength: &HashMap<NodeIndex, f64>,
    prefer_files: bool,
    token_budget: i64,
    skip_records: usize,
    header: &str,
) -> astria_core::Result<(String, Option<usize>)> {
    // Relevance-ranked, not hub-ranked: question-matched seeds surface
    // first, then nodes by traversal distance to those seeds, and only
    // then by degree. Pure degree ordering buried the files the question
    // was actually about beneath graph-wide hubs.
    let mut node_list: Vec<NodeIndex> = visited.iter().copied().collect();
    node_list.sort_by(|&a, &b| {
        let na = &loaded.graph[a];
        let nb = &loaded.graph[b];
        let sa = relevance.get(&a).copied().unwrap_or(0.0);
        let sb = relevance.get(&b).copied().unwrap_or(0.0);
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            // Weakly-reached nodes (best touching edge below the semantic
            // floor) come after strongly-reached ones at equal relevance.
            // Plain graphs hold no such nodes: their edges sit at 0.7+.
            .then_with(|| {
                let wa = reach_strength.get(&a).copied().unwrap_or(1.0);
                let wb = reach_strength.get(&b).copied().unwrap_or(1.0);
                (wa < SEMANTIC_WEAK_FLOOR).cmp(&(wb < SEMANTIC_WEAK_FLOOR))
            })
            .then_with(|| {
                distance
                    .get(&a)
                    .copied()
                    .unwrap_or(u32::MAX)
                    .cmp(&distance.get(&b).copied().unwrap_or(u32::MAX))
            })
            .then_with(|| {
                loaded
                    .graph
                    .neighbors(b)
                    .count()
                    .cmp(&loaded.graph.neighbors(a).count())
            })
            .then_with(|| {
                if prefer_files {
                    let fa = label_is_file(&na.label);
                    let fb = label_is_file(&nb.label);
                    fb.cmp(&fa) // file nodes before symbols at equal relevance
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| na.label.cmp(&nb.label))
            .then_with(|| na.id.cmp(&nb.id))
    });

    let mut records = Vec::new();
    for idx in &node_list {
        let idx = *idx;
        let node = &loaded.graph[idx];
        let comm = node.community.map_or("?".to_string(), |c| c.to_string());
        let loc = match node.source_line {
            Some(line) => format!("{}:{}", loaded.display_path(&node.source_file), line),
            None => loaded.display_path(&node.source_file),
        };
        let mut line = format!(
            "NODE {} [id={} src={} community={}]\n",
            node.label, node.id, loc, comm
        );
        // Chunked bodies cite their covered line range so agents can quote
        // exact spans; harness parsers only read the src= token, so the
        // range rides on its own line.
        if node.file_type == "chunk" {
            if let (Some(start), Some(doc)) = (node.source_line, &node.docstring) {
                let end = start + doc.lines().count() as i64 - 1;
                line.push_str(&format!("  span: L{start}-L{end}\n"));
            }
        }
        if let Some(sig) = &node.signature {
            let short: String = sig.chars().take(140).collect();
            line.push_str(&format!("  sig: {}\n", short));
        } else if let Some(ref doc) = node.docstring {
            if !doc.is_empty() {
                let summary: String = doc.chars().take(200).collect();
                line.push_str(&format!("  summary: {}\n", summary));
            }
        }
        records.push(line);
    }
    let mut edge_records = Vec::new();
    let mut edge_list = edges_seen.to_vec();
    edge_list.sort_by(|&a, &b| {
        let key = |edge| {
            let (source, target) = loaded.graph.edge_endpoints(edge).unwrap();
            let score = relevance
                .get(&source)
                .copied()
                .unwrap_or(0.0)
                .max(relevance.get(&target).copied().unwrap_or(0.0));
            (
                score,
                &loaded.graph[source].id,
                &loaded.graph[target].id,
                &loaded.graph[edge].relation,
            )
        };
        let ka = key(a);
        let kb = key(b);
        kb.0.total_cmp(&ka.0)
            .then_with(|| ka.1.cmp(kb.1))
            .then_with(|| ka.2.cmp(kb.2))
            .then_with(|| ka.3.cmp(kb.3))
            .then_with(|| a.index().cmp(&b.index()))
    });
    for &edge_id in &edge_list {
        if let Some((src_idx, tgt_idx)) = loaded.graph.edge_endpoints(edge_id) {
            let src = &loaded.graph[src_idx];
            let tgt = &loaded.graph[tgt_idx];
            let edge = &loaded.graph[edge_id];
            let loc = match edge.source_line {
                Some(l) => format!(" @{}:{}", loaded.display_path(&edge.source_file), l),
                None => String::new(),
            };
            let score = edge
                .confidence_score
                .map(|s| format!(":{s:.2}"))
                .unwrap_or_default();
            let line = format!(
                "EDGE {} --{} [{}{}]--> {}{}\n",
                src.label, edge.relation, edge.confidence, score, tgt.label, loc
            );
            edge_records.push(line);
        }
    }
    // Fixed interleaving keeps relationships on the first page while the
    // cursor still addresses every complete node and edge exactly once.
    let mut interleaved = Vec::with_capacity(records.len() + edge_records.len());
    let mut nodes = records.into_iter();
    let mut edges = edge_records.into_iter();
    loop {
        let before = interleaved.len();
        interleaved.extend(nodes.by_ref().take(2));
        interleaved.extend(edges.by_ref().take(1));
        if interleaved.len() == before {
            break;
        }
    }
    render_page(header, &interleaved, skip_records, token_budget)
}

/// Public output contract: o200k_base, ordinary text (special-looking strings
/// are encoded literally). All headers, timestamps and pagination count.
pub fn count_response_tokens(text: &str) -> usize {
    tiktoken_rs::o200k_base_singleton()
        .encode_ordinary(text)
        .len()
}

/// Count manifest files modified after the graph was published (with a
/// small skew so same-second writes do not cry stale). Stat-only: this runs
/// on every query. `None` when the manifest is unreadable — disclosure is
/// best-effort and must never fail a query.
fn files_changed_since(db: &Connection, built_at_secs: u64) -> Option<usize> {
    let cutoff = std::time::UNIX_EPOCH + std::time::Duration::from_secs(built_at_secs + 2);
    let mut stmt = db.prepare("SELECT file_path FROM file_manifest").ok()?;
    let paths: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .ok()?
        .flatten()
        .collect();
    let mut changed = 0usize;
    for path in &paths {
        if let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) {
            if modified > cutoff {
                changed += 1;
            }
        }
    }
    Some(changed)
}

fn render_page(
    header: &str,
    records: &[String],
    cursor: usize,
    budget: i64,
) -> astria_core::Result<(String, Option<usize>)> {
    let limit = usize::try_from(budget)
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| {
            astria_core::AstriaError::Graph(
                "budget must be a positive o200k_base token count".into(),
            )
        })?;
    if cursor > records.len() {
        return Err(astria_core::AstriaError::Graph(format!(
            "cursor {cursor} exceeds {} records",
            records.len()
        )));
    }
    let mut body = String::new();
    let mut best = None;
    for end in cursor..=records.len() {
        let next = (end < records.len()).then_some(end);
        let footer = next
            .map(|n| format!("\n(continuation: re-run with cursor {n} for the next records)\n"))
            .unwrap_or_default();
        let text = format!("{header}{body}{footer}");
        if count_response_tokens(&text) > limit {
            break;
        }
        if end > cursor || end == records.len() {
            best = Some((text, next));
        }
        if let Some(record) = records.get(end) {
            body.push_str(record);
        }
    }
    // A final page has no continuation footer. Even if the footer alone
    // does not fit, the remaining complete response may still fit.
    if best.is_none() {
        let final_page = format!("{header}{}", records[cursor..].concat());
        if count_response_tokens(&final_page) <= limit {
            return Ok((final_page, None));
        }
    }
    best.ok_or_else(|| {
        astria_core::AstriaError::Graph(
            "budget too small for the response metadata and next complete record; increase budget"
                .into(),
        )
    })
}

fn shortest_path_bfs(
    loaded: &LoadedGraph,
    start: NodeIndex,
    end: NodeIndex,
    directed: bool,
    min_strength: f64,
    semantic_floor: f64,
) -> Option<Vec<EdgeIndex>> {
    if start == end {
        return Some(Vec::new());
    }
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut parent: HashMap<NodeIndex, (NodeIndex, EdgeIndex)> = HashMap::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(start);
    visited.insert(start);

    while let Some(current) = queue.pop_front() {
        for (neighbor, edge_id) in iter_neighbors_filtered(
            &loaded.graph,
            current,
            directed,
            min_strength,
            semantic_floor,
        ) {
            if visited.contains(&neighbor) {
                continue;
            }
            parent.insert(neighbor, (current, edge_id));
            if neighbor == end {
                let mut path = Vec::new();
                let mut cur = end;
                while let Some(&(p, edge_id)) = parent.get(&cur) {
                    path.push(edge_id);
                    cur = p;
                }
                path.reverse();
                return Some(path);
            }
            visited.insert(neighbor);
            queue.push_back(neighbor);
        }
    }
    None
}

/// Rendered text, node count, edge count, and pagination cursor.
pub type QueryOutput = (String, usize, usize, Option<usize>);

/// Traversal query. `min_strength` is the fidelity tier (0.0 = all facts;
/// 0.9 = EXTRACTED/DECLARED only); `cursor` continues a previously
/// truncated node list. Returns (text, nodes, edges, next_cursor) —
/// `next_cursor` is Some when more ranked nodes remain.
#[allow(clippy::too_many_arguments)]
pub fn query_graph(
    db: &Connection,
    db_path: &str,
    question: &str,
    mode: &str,
    depth: usize,
    budget: i64,
    directed: bool,
    min_strength: f64,
    cursor: usize,
    prefer_files: bool,
) -> astria_core::Result<(String, usize, usize, Option<usize>)> {
    query_graph_with_metadata(
        db,
        db_path,
        question,
        mode,
        depth,
        budget,
        directed,
        min_strength,
        cursor,
        prefer_files,
    )
    .map(|(result, _)| result)
}

/// Query output and build timestamp read from the same SQLite generation.
#[allow(clippy::too_many_arguments)]
pub fn query_graph_with_metadata(
    db: &Connection,
    db_path: &str,
    question: &str,
    mode: &str,
    depth: usize,
    budget: i64,
    directed: bool,
    min_strength: f64,
    cursor: usize,
    prefer_files: bool,
) -> astria_core::Result<(QueryOutput, Option<String>)> {
    let transaction = read_snapshot(db)?;
    let loaded = load_graph(db, db_path)?;
    let graph_built_at = db
        .query_row(
            "SELECT value FROM _meta WHERE key = 'graph_published_at'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok();
    // Only use an already cached model: queries never initiate a download.
    #[cfg(feature = "embed")]
    let semantic = if !semantic_seeds_disabled()
        && astria_embed::has_embeddings(db)
        && astria_embed::model_cached()
    {
        match astria_embed::load_embedder() {
            Ok(mut model) => astria_embed::semantic_scores(db, &mut model, question)
                .unwrap_or_else(|e| {
                    // Silent empty seeds made --embed look like a no-op; a
                    // query-side failure must be visible.
                    eprintln!("warning: embedding seeds unavailable: {e}");
                    Vec::new()
                }),
            Err(e) => {
                eprintln!("warning: embedding model unavailable: {e}");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    #[cfg(not(feature = "embed"))]
    let semantic: Vec<(String, f64)> = Vec::new();
    if let Some(transaction) = transaction {
        transaction.commit()?;
    }
    query_graph_loaded(
        db,
        loaded,
        question,
        mode,
        depth,
        budget,
        directed,
        min_strength,
        cursor,
        &semantic,
        prefer_files,
        graph_built_at.as_deref(),
    )
    .map(|result| (result, graph_built_at))
}

/// Rescale a raw query→node cosine into the seed-score scale via the
/// measured calibration: the noise floor scores 0, a strong match reaches
/// the cap (just under an exact label match, so token evidence always
/// outranks pure embedding recall), mid-range matches land near the
/// path/docstring layer. The anchors are model-specific — see
/// `astria_core::calibration`.
fn semantic_seed_score(cosine: f64) -> f64 {
    astria_core::calibration::SEMANTIC_CALIBRATION.seed_score(cosine)
}

/// `ASTRIA_EMBED=off|0|false|no` stops queries from auto-merging embedding
/// seeds. Build-side `--embed` still computes vectors; this only turns off
/// consuming them, so structural behavior can be measured or enforced on a
/// graph that carries vectors.
#[cfg_attr(not(feature = "embed"), allow(dead_code))]
fn semantic_seeds_disabled() -> bool {
    semantic_seeds_disabled_value(std::env::var("ASTRIA_EMBED").ok().as_deref())
}

#[cfg_attr(not(feature = "embed"), allow(dead_code))]
fn semantic_seeds_disabled_value(value: Option<&str>) -> bool {
    match value {
        Some(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        ),
        None => false,
    }
}

/// `ASTRIA_QUERY_SEED_FLOOR=off|0|false|no` disables the no-confident-match
/// guard so a traversal always runs, however weak the seed evidence — the
/// historical behavior, kept reachable for measurement and IR-style recall.
fn seed_floor_enabled() -> bool {
    seed_floor_enabled_value(std::env::var("ASTRIA_QUERY_SEED_FLOOR").ok().as_deref())
}

fn seed_floor_enabled_value(value: Option<&str>) -> bool {
    !semantic_seeds_disabled_value(value)
}

/// `query_graph` plus semantic seed candidates: `(node_id, cosine)` pairs
/// from an embedding model. Semantic matches merge with token scoring by
/// taking each node's best score, so a purely semantic match still seeds
/// the traversal — how "auth flow" finds `SessionMiddleware` with zero
/// string overlap — while exact label matches keep outranking it.
#[allow(clippy::too_many_arguments)]
pub fn query_graph_with_semantic(
    db: &Connection,
    db_path: &str,
    question: &str,
    mode: &str,
    depth: usize,
    budget: i64,
    directed: bool,
    min_strength: f64,
    cursor: usize,
    semantic: &[(String, f64)],
    prefer_files: bool,
) -> astria_core::Result<(String, usize, usize, Option<usize>)> {
    let loaded = load_graph_snapshot(db, db_path)?;
    query_graph_loaded(
        db,
        loaded,
        question,
        mode,
        depth,
        budget,
        directed,
        min_strength,
        cursor,
        semantic,
        prefer_files,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn query_graph_loaded(
    db: &Connection,
    loaded: LoadedGraph,
    question: &str,
    mode: &str,
    depth: usize,
    budget: i64,
    directed: bool,
    min_strength: f64,
    cursor: usize,
    semantic: &[(String, f64)],
    prefer_files: bool,
    graph_built_at: Option<&str>,
) -> astria_core::Result<(String, usize, usize, Option<usize>)> {
    if loaded.graph.node_count() == 0 {
        let (text, _) = render_page("", &["No nodes in graph.\n".into()], 0, budget)?;
        return Ok((text, 0, 0, None));
    }

    let terms: Vec<String> = question.split_whitespace().map(|s| s.to_string()).collect();
    let scored_nodes = score_nodes(&loaded, &terms);
    let mut scored = scored_nodes.ranked;
    // Opt-in hard floor for SEMANTIC edges; 0.0 keeps every edge (the
    // historical behavior). Read per query so agents can retune without
    // a restart.
    let semantic_floor = std::env::var("ASTRIA_QUERY_MIN_SEMANTIC_CONFIDENCE")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);

    // Merge semantic candidates: max(token score, rescaled cosine) per node.
    // Token evidence is snapshotted first so semantic-only candidates (nodes
    // the embeddings surface but no query term touches) stay identifiable —
    // they qualify for the seed reservation below.
    //
    // Description-shaped questions — the ones whose identifying (salient)
    // terms have no lexical evidence anywhere, like "auth flow" against a
    // graph that never uses those words — are where embeddings are the
    // whole point, and there a strong calibrated cosine ranks like a label
    // match instead of capping below every partial token match. When the
    // graph does answer the question's vocabulary, the tie-breaking cap
    // stands: token evidence always outranks pure embedding recall.
    let description_shaped = scored_nodes.max_salient_hits == 0;
    let token_scores: std::collections::HashMap<NodeIndex, f64> = if semantic.is_empty() {
        std::collections::HashMap::new()
    } else {
        scored.iter().map(|(s, i)| (*i, *s)).collect()
    };
    if !semantic.is_empty() {
        let mut by_index: std::collections::HashMap<NodeIndex, f64> =
            scored.iter().map(|(s, i)| (*i, *s)).collect();
        for (node_id, cosine) in semantic {
            if let Some(&idx) = loaded.id_to_idx.get(node_id) {
                let rescaled = if description_shaped {
                    astria_core::calibration::SEMANTIC_CALIBRATION.description_seed_score(*cosine)
                } else {
                    semantic_seed_score(*cosine)
                };
                let entry = by_index.entry(idx).or_insert(0.0);
                *entry = (*entry).max(rescaled);
            }
        }
        scored = by_index.into_iter().map(|(i, s)| (s, i)).collect();
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| loaded.graph[a.1].label.cmp(&loaded.graph[b.1].label))
                .then_with(|| loaded.graph[a.1].id.cmp(&loaded.graph[b.1].id))
        });
    }

    if scored.is_empty() {
        let suggestions = nearest_labels(&loaded, question, SUGGESTION_COUNT);
        let msg = if suggestions.is_empty() {
            "No matching nodes found.".to_string()
        } else {
            format!(
                "No matching nodes found. Did you mean: {}?",
                suggestions.join(", ")
            )
        };
        let (text, _) = render_page("", &[format!("{msg}\n")], 0, budget)?;
        return Ok((text, 0, 0, None));
    }

    // Seed-confidence floor: when no node in the graph matches any of the
    // query's salient (highest-IDF, answer-identifying) terms AND no node
    // matched even half of the effective terms, every hit is an incidental
    // word match — "OAuth handled" seeding handle_message() off the stem
    // "handl", or a payroll question seeding index.module.css on "module" —
    // and a full-budget traversal from it is authoritative noise. Return an
    // explicit miss naming the missing vocabulary instead. The half-of-terms
    // bar is what separates a weak-but-on-topic match (SessionManager on
    // "session handling") from an incidental one (one common word of five):
    // a single ubiquitous term fully covering one label is real evidence,
    // but it does not identify an answer. Entry-intent queries are exempt —
    // their terms match nothing by design and the import DAG answers them.
    // A qualifying semantic-only candidate overrides the floor: conceptual
    // questions with zero vocabulary overlap are exactly what embeddings
    // are for. Docs-majority corpora (transcripts, QA datasets) run on weak
    // lexical matches by design, so the floor applies to code-majority
    // graphs only. ASTRIA_QUERY_SEED_FLOOR=off restores the historical
    // always-traverse behavior.
    let docs_majority = matches!(corpus_mode(&loaded), CorpusMode::DocsMajority);
    // Kept for the corpus-mode disclosure header below (pin vs auto).
    let corpus_pin = corpus_mode_pin();
    let semantic_qualifies = semantic
        .iter()
        .any(|(_, cosine)| semantic_seed_score(*cosine) >= SEMANTIC_SEED_FLOOR);
    if seed_floor_enabled()
        && !semantic_qualifies
        && !docs_majority
        && !scored_nodes.entry_intent
        && !scored_nodes.salient_terms.is_empty()
        && scored_nodes.max_salient_hits == 0
        && scored_nodes.max_matched_terms * 2 < scored_nodes.effective_count
    {
        let suggestions = nearest_labels(&loaded, question, SUGGESTION_COUNT);
        let missing = scored_nodes
            .missing_terms
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let mut msg = format!(
            "No confident match: none of the question's key terms ({missing}) appear in the graph's labels, docstrings, or ids, so every candidate is an incidental word match. Try a symbol name, a file path, or vocabulary the code itself uses."
        );
        if !suggestions.is_empty() {
            msg.push_str(&format!(" Did you mean: {}?", suggestions.join(", ")));
        }
        let (text, _) = render_page("", &[format!("{msg}\n")], 0, budget)?;
        return Ok((text, 0, 0, None));
    }

    // Seed quota: at most 2 of 5 seeds may be documentation-type nodes.
    // Doc headings keyword-match almost as strongly as code, and when they
    // dominate the seed set the traversal starts in prose and never reaches
    // the implementing crate file. Code/pattern/package seeds are unbounded.
    let mut seed_nodes: Vec<NodeIndex> = Vec::new();
    // Reserve one candidate per explicitly named identifier or scope when a
    // full match exists. Weak partial words cannot claim coverage.
    for term in &terms {
        if !is_explicit_identifier(term) || term.len() <= 2 {
            continue;
        }
        let parts = tokenize(term);
        if let Some(&(_, idx)) = scored.iter().find(|(_, idx)| {
            let node = &loaded.graph[*idx];
            if node.file_type == "stub" {
                return false;
            }
            normalized_identifier(term) == normalized_identifier(&node.label)
                || (parts.len() > 1 && component_coverage(&parts, &tokenize(&node.label)) == 1.0)
                || node
                    .source_file
                    .replace('\\', "/")
                    .split('/')
                    .any(|segment| normalized_identifier(segment) == normalized_identifier(term))
                || qualified_scope_match(term, &node.id)
        }) {
            if !seed_nodes.contains(&idx) {
                seed_nodes.push(idx);
            }
        }
    }
    let seed_limit = 5.max(seed_nodes.len());
    // Doc-seed quota: on code corpora two prose seeds already outrank
    // silence, and more would crowd code out of the seed set. On
    // docs-majority graphs (transcript corpora, docs sites with no code)
    // the quota would leave the traversal nearly seedless — there is no
    // code to protect, so the quota opens up to the full seed limit.
    let doc_seed_quota = if docs_majority { seed_limit } else { 2 };
    let mut doc_seeds = seed_nodes
        .iter()
        .filter(|&&idx| {
            is_doc_type(&loaded.graph[idx].file_type)
                || is_semantic_type(&loaded.graph[idx].file_type)
        })
        .count();
    for &(_, idx) in scored.iter() {
        if seed_nodes.len() == seed_limit {
            break;
        }
        if seed_nodes.contains(&idx) {
            continue;
        }
        if is_doc_type(&loaded.graph[idx].file_type)
            || is_semantic_type(&loaded.graph[idx].file_type)
        {
            if doc_seeds >= doc_seed_quota && !wants_docs(&terms) {
                continue;
            }
            doc_seeds += 1;
        }
        seed_nodes.push(idx);
    }
    // Semantic-only reservation: embedding recall must be able to start the
    // traversal when the question shares no vocabulary with the answer —
    // its advertised purpose ("conceptual questions with zero string
    // overlap still find their symbols"). Such a node's rescaled cosine
    // (~0-0.67) never outranks token matches (2+), so without this slot
    // --embed is inert exactly where it matters. One slot for the best
    // qualifying node: embedding evidence above the floor, zero token
    // evidence, not a stub.
    if !semantic.is_empty() && seed_limit >= 3 {
        let best = semantic
            .iter()
            .filter_map(|(node_id, cosine)| {
                let &idx = loaded.id_to_idx.get(node_id)?;
                if seed_nodes.contains(&idx) || loaded.graph[idx].file_type == "stub" {
                    return None;
                }
                if token_scores.get(&idx).copied().unwrap_or(0.0) > 0.0 {
                    return None;
                }
                let score = semantic_seed_score(*cosine);
                (score >= SEMANTIC_SEED_FLOOR).then_some((score, idx))
            })
            .max_by(|a, b| {
                a.0.partial_cmp(&b.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.1.cmp(&a.1))
            });
        if let Some((_, idx)) = best {
            seed_nodes.push(idx);
        }
    }
    if debug_scores_enabled() {
        let seeds: Vec<String> = seed_nodes
            .iter()
            .map(|&idx| truncate_label(&loaded.graph[idx].id))
            .collect();
        eprintln!("-- seeds ({}): {:?} --", seed_nodes.len(), seeds);
    }
    let (visited, edges_seen, distance) = if mode == "dfs" {
        dfs_subgraph(
            &loaded,
            &seed_nodes,
            depth,
            directed,
            min_strength,
            semantic_floor,
        )
    } else {
        bfs_subgraph(
            &loaded,
            &seed_nodes,
            depth,
            directed,
            min_strength,
            semantic_floor,
        )
    };

    // Weak-tier detection: a visited node whose strongest touching edge is
    // below the floor was reached only through low-confidence SEMANTIC
    // links (structural and inferred edges sit at 0.7+). Such nodes sort
    // after every strongly-reached node, so budget truncation drops
    // speculative links first — the judge's calibrated verdicts shaping
    // what survives the token budget.
    let mut reach_strength: HashMap<NodeIndex, f64> = HashMap::new();
    for &edge_id in &edges_seen {
        if let Some((s, t)) = loaded.graph.edge_endpoints(edge_id) {
            let st = loaded.graph[edge_id].strength();
            for n in [s, t] {
                let entry = reach_strength.entry(n).or_insert(0.0);
                if st > *entry {
                    *entry = st;
                }
            }
        }
    }

    let seed_labels: Vec<String> = seed_nodes
        .iter()
        .map(|&idx| loaded.graph[idx].label.clone())
        .collect();

    let mut header = format!(
        "Traversal: {} depth={}{} | Start: {:?} | {} nodes found\n\n",
        mode.to_uppercase(),
        depth,
        if directed { " directed" } else { "" },
        seed_labels,
        visited.len()
    );
    if let Some(timestamp) = graph_built_at {
        header.push_str(&format!("# graph built at {timestamp}\n"));
        // Staleness disclosure: hooked editors and the git hook keep the
        // graph fresh, but edits through other paths (print-mode sessions,
        // editors without hooks, plain typing) do not — and an agent that
        // does not notice the timestamp answers from the past. Stat-only
        // (no re-hashing), so this costs milliseconds even on large repos.
        if let Ok(built) = timestamp.parse::<u64>() {
            if let Some(changed) = files_changed_since(db, built) {
                if changed > 0 {
                    header.push_str(&format!(
                        "# {changed} file(s) changed since this build — run `astria update` before trusting answers\n"
                    ));
                }
            }
        }
    }
    // Corpus-mode disclosure: the ranking rules differ between modes, so a
    // mode other than auto-detected code-majority is stated in the header
    // instead of silently changing the answer's shape.
    if docs_majority || corpus_pin.is_some() {
        let mode_name = if docs_majority {
            "docs-majority"
        } else {
            "code-majority"
        };
        let pin_note = if corpus_pin.is_some() {
            ", pinned via ASTRIA_CORPUS_MODE"
        } else {
            ""
        };
        header.push_str(&format!(
            "# corpus: {mode_name} ({:.0}% prose{pin_note})\n",
            prose_share(&loaded) * 100.0
        ));
    }
    let relevance: HashMap<NodeIndex, f64> = scored.iter().map(|(s, i)| (*i, *s)).collect();
    let (result_text, next_cursor) = subgraph_to_text(
        &loaded,
        &visited,
        &edges_seen,
        &relevance,
        &distance,
        &reach_strength,
        prefer_files,
        budget,
        cursor,
        &header,
    )?;

    log_query(db, question, &result_text);
    record_query_pairs(db, &loaded, &seed_nodes, &visited, question);

    Ok((result_text, visited.len(), edges_seen.len(), next_cursor))
}

/// Feedback loop bookkeeping: for one answered query, record which
/// (seed, discovered) node pairs the traversal connected. Pairs recurring
/// across DISTINCT questions later promote into `learned` edges — the
/// graph remembers which connections users actually keep asking about.
fn record_query_pairs(
    db: &Connection,
    loaded: &LoadedGraph,
    seeds: &[NodeIndex],
    visited: &HashSet<NodeIndex>,
    question: &str,
) {
    let question: String = question
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect();
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

    let pairs: Vec<(String, String, i64)> = {
        let mut stmt = db.prepare(
            "SELECT source, target, SUM(hits) FROM query_pairs
             GROUP BY source, target
             HAVING COUNT(DISTINCT question) >= ?1 AND SUM(hits) >= ?2",
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

pub fn find_shortest_path(
    db: &Connection,
    db_path: &str,
    source_query: &str,
    target_query: &str,
    directed: bool,
    min_strength: f64,
) -> astria_core::Result<(bool, usize, String)> {
    let transaction = read_snapshot(db)?;
    let loaded = load_graph(db, db_path)?;
    if loaded.graph.node_count() == 0 {
        return Ok((false, 0, "No nodes in graph.".to_string()));
    }

    // Exact ids win over fuzzy scoring, and a stub never shadows a
    // same-named definition (same rule as affected/explain seeds). Scoring
    // stays as the fallback for natural-language endpoints.
    let resolve_endpoint = |query: &str| -> Option<NodeIndex> {
        if let Some(&idx) = loaded.id_to_idx.get(query) {
            let is_stub: bool = db
                .query_row(
                    "SELECT file_type = 'stub' FROM nodes WHERE id = ?1",
                    rusqlite::params![query],
                    |r| r.get(0),
                )
                .unwrap_or(false);
            if !is_stub {
                return Some(idx);
            }
            let bare = query
                .trim_start_matches('.')
                .trim_end_matches("()")
                .to_lowercase();
            if let Some(id) = astria_core::db::prefer_non_stub_id(db, &bare) {
                if let Some(&better) = loaded.id_to_idx.get(id.as_str()) {
                    return Some(better);
                }
            }
            return Some(idx);
        }
        None
    };

    let src_idx = match resolve_endpoint(source_query) {
        Some(idx) => idx,
        None => {
            let src_terms: Vec<String> = source_query
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            let src_scored = score_nodes(&loaded, &src_terms);
            match src_scored.ranked.first() {
                Some((_, idx)) => *idx,
                None => {
                    let mut msg = format!("No matching node for '{}'.", source_query);
                    let suggestions = nearest_labels(&loaded, source_query, SUGGESTION_COUNT);
                    if !suggestions.is_empty() {
                        msg.push_str(&format!(" Did you mean: {}?", suggestions.join(", ")));
                    }
                    return Ok((false, 0, msg));
                }
            }
        }
    };
    let tgt_idx = match resolve_endpoint(target_query) {
        Some(idx) => idx,
        None => {
            let tgt_terms: Vec<String> = target_query
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            let tgt_scored = score_nodes(&loaded, &tgt_terms);
            match tgt_scored.ranked.first() {
                Some((_, idx)) => *idx,
                None => {
                    let mut msg = format!("No matching node for '{}'.", target_query);
                    let suggestions = nearest_labels(&loaded, target_query, SUGGESTION_COUNT);
                    if !suggestions.is_empty() {
                        msg.push_str(&format!(" Did you mean: {}?", suggestions.join(", ")));
                    }
                    return Ok((false, 0, msg));
                }
            }
        }
    };

    let semantic_floor = std::env::var("ASTRIA_QUERY_MIN_SEMANTIC_CONFIDENCE")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let path = match shortest_path_bfs(
        &loaded,
        src_idx,
        tgt_idx,
        directed,
        min_strength,
        semantic_floor,
    ) {
        Some(p) => p,
        None => return Ok((false, 0, "No path found.".to_string())),
    };

    let hops = path.len();
    let mut text = format!("Shortest path ({} hops):\n", hops);

    for edge_id in path {
        let (source, target) = loaded
            .graph
            .edge_endpoints(edge_id)
            .expect("traversed edge exists");
        let edge = &loaded.graph[edge_id];
        text.push_str(&format!(
            "  {} --{} [{}]--> {}\n",
            loaded.graph[source].label, edge.relation, edge.confidence, loaded.graph[target].label
        ));
    }

    if let Some(transaction) = transaction {
        transaction.commit()?;
    }
    let answer = format!("path found: {} hops", hops);
    log_query(
        db,
        &format!("{} -> {}", source_query, target_query),
        &answer,
    );

    Ok((true, hops, text))
}

/// Aider-style repo map: files ranked by PageRank over the file-level
/// reference graph, each with its most-connected symbols. One budgeted
/// blob that orients an agent over the whole repo — the "orient for a
/// fixed token cost" artifact that replaces scattered file reading.
pub fn repo_map(
    db: &Connection,
    db_path: &str,
    budget: i64,
    min_strength: f64,
) -> astria_core::Result<(String, usize)> {
    let loaded = load_graph_snapshot(db, db_path)?;
    if loaded.graph.node_count() == 0 {
        return Ok(("No nodes in graph.".to_string(), 0));
    }

    // Display-form file of each node (indexed by NodeIndex::index()).
    let file_of: Vec<String> = loaded
        .graph
        .node_indices()
        .map(|idx| loaded.display_path(&loaded.graph[idx].source_file))
        .collect();
    let mut files: Vec<String> = file_of.clone();
    files.sort();
    files.dedup();
    let n = files.len();
    let file_rank: HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.as_str(), i))
        .collect();

    // File-level adjacency: undirected weight = cross-file edge count.
    let mut adj: Vec<HashMap<usize, f64>> = vec![HashMap::new(); n];
    let mut out_sum: Vec<f64> = vec![0.0; n];
    for e in loaded.graph.edge_references() {
        if !e.weight().meets_detail(min_strength) {
            continue;
        }
        let sf = &file_of[e.source().index()];
        let tf = &file_of[e.target().index()];
        let (a, b) = (file_rank[sf.as_str()], file_rank[tf.as_str()]);
        if a != b {
            *adj[a].entry(b).or_insert(0.0) += 1.0;
            *adj[b].entry(a).or_insert(0.0) += 1.0;
            out_sum[a] += 1.0;
            out_sum[b] += 1.0;
        }
    }

    // PageRank with dangling-mass redistribution.
    let damping = 0.85_f64;
    let mut rank: Vec<f64> = vec![1.0 / n as f64; n];
    for _ in 0..30 {
        let dangling: f64 = (0..n).filter(|&i| out_sum[i] <= 0.0).map(|i| rank[i]).sum();
        let mut next = vec![(1.0 - damping) / n as f64 + damping * dangling / n as f64; n];
        for i in 0..n {
            if out_sum[i] <= 0.0 {
                continue;
            }
            let share = damping * rank[i] / out_sum[i];
            for (&j, &w) in &adj[i] {
                next[j] += share * w;
            }
        }
        rank = next;
    }

    // Top symbols per file by degree (deterministic ties by label).
    let mut file_symbols: Vec<Vec<(NodeIndex, usize)>> = vec![Vec::new(); n];
    for idx in loaded.graph.node_indices() {
        let fi = file_rank[file_of[idx.index()].as_str()];
        file_symbols[fi].push((idx, loaded.graph.neighbors(idx).count()));
    }
    for syms in &mut file_symbols {
        syms.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| loaded.graph[a.0].label.cmp(&loaded.graph[b.0].label))
        });
        syms.truncate(3);
    }

    // Emit within budget.
    let char_budget = (budget.max(1) as usize) * 3;
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| {
        rank[*b]
            .partial_cmp(&rank[*a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| files[*a].cmp(&files[*b]))
    });

    let mut out = format!("Repo map ({} files, PageRank-ranked):\n", n);
    let mut shown = 0usize;
    for &fi in &order {
        let mut block = format!("\n{} (rank {:.4})\n", files[fi], rank[fi]);
        for (idx, deg) in &file_symbols[fi] {
            let node = &loaded.graph[*idx];
            block.push_str(&format!(
                "  - {} [id={}] (degree {})\n",
                node.label, node.id, deg
            ));
        }
        if out.len() + block.len() > char_budget && shown > 0 {
            out.push_str(&format!(
                "\n... (map truncated: {} of {} files; raise --budget for more)\n",
                shown, n
            ));
            break;
        }
        out.push_str(&block);
        shown += 1;
    }

    Ok((out, shown))
}

pub fn explain_with_neighbors(
    db: &Connection,
    db_path: &str,
    node_id: &str,
) -> astria_core::Result<Option<ExplainResult>> {
    let transaction = read_snapshot(db)?;
    let loaded = load_graph(db, db_path)?;

    // A stub must not shadow a same-named real definition: explaining by a
    // bare name would otherwise land on a speculative node (no edges, no
    // provenance) instead of the symbol.
    let resolved_id = {
        let bare = node_id
            .trim_start_matches('.')
            .trim_end_matches("()")
            .to_lowercase();
        let exact_is_stub: bool = db
            .query_row(
                "SELECT file_type = 'stub' FROM nodes WHERE id = ?1",
                rusqlite::params![node_id],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if exact_is_stub {
            astria_core::db::prefer_non_stub_id(db, &bare).unwrap_or_else(|| node_id.to_string())
        } else {
            node_id.to_string()
        }
    };

    let idx = match loaded.id_to_idx.get(resolved_id.as_str()) {
        Some(&idx) => idx,
        None => {
            let terms: Vec<String> = resolved_id
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            let scored = score_nodes(&loaded, &terms);
            match scored.ranked.first() {
                Some((_, idx)) => *idx,
                None => return Ok(None),
            }
        }
    };

    let node = &loaded.graph[idx];
    // Explain is a lookup, not a traversal: neighbors in both directions.
    let mut seen: HashSet<NodeIndex> = HashSet::new();
    let mut neighbors: Vec<EdgeInfoResult> = Vec::new();
    for neighbor in iter_neighbors(&loaded.graph, idx, false) {
        if !seen.insert(neighbor) {
            continue;
        }
        let neighbor_data = &loaded.graph[neighbor];
        // The stored orientation says which way the edge points: outgoing
        // (this node → neighbor, e.g. it calls the neighbor) or incoming
        // (neighbor → this node, e.g. the neighbor calls it). Rendering
        // every connection as if the explained node were the source
        // inverts caller/callee and misleads agents reading it.
        let forward = loaded
            .graph
            .edges_directed(idx, Direction::Outgoing)
            .find(|e| e.target() == neighbor)
            .map(|e| e.weight());
        let outgoing = forward.is_some();
        let edge = forward.or_else(|| edge_between(&loaded.graph, idx, neighbor));
        neighbors.push(EdgeInfoResult {
            neighbor_id: neighbor_data.id.clone(),
            neighbor_label: neighbor_data.label.clone(),
            neighbor_file: loaded.display_path(&neighbor_data.source_file),
            neighbor_line: neighbor_data.source_line,
            outgoing,
            relation: edge.map_or("?".to_string(), |e| e.relation.clone()),
            confidence: edge.map_or("?".to_string(), |e| e.confidence.clone()),
            strength: edge.map_or(0.0, |e| e.strength()),
            confidence_score: edge.and_then(|e| e.confidence_score),
        });
    }

    // Strongest connections first; ties broken deterministically.
    neighbors.sort_by(|a, b| {
        b.strength
            .partial_cmp(&a.strength)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.relation.cmp(&b.relation))
            .then_with(|| a.neighbor_id.cmp(&b.neighbor_id))
    });
    let neighbor_count = neighbors.len();
    neighbors.truncate(20);

    let answer = format!("explain: {} ({} neighbors)", node.label, neighbor_count);

    // Hyperedge membership: which N-ary groups this node belongs to.
    let hyperedges: Vec<String> = {
        let mut stmt = db.prepare(
            "SELECT label FROM hyperedges WHERE EXISTS (
               SELECT 1 FROM json_each(hyperedges.nodes) WHERE json_each.value = ?1
             ) LIMIT 5",
        )?;
        let rows = stmt.query_map(rusqlite::params![node.id], |r| r.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    if let Some(transaction) = transaction {
        transaction.commit()?;
    }
    log_query(db, node_id, &answer);

    Ok(Some(ExplainResult {
        id: node.id.clone(),
        label: node.label.clone(),
        source_file: loaded.display_path(&node.source_file),
        source_line: node.source_line,
        community: node.community,
        neighbor_count,
        neighbors,
        hyperedges,
    }))
}

pub struct EdgeInfoResult {
    pub neighbor_id: String,
    pub neighbor_label: String,
    pub neighbor_file: String,
    pub neighbor_line: Option<i64>,
    /// True when the stored edge points from the explained node to this
    /// neighbor (it calls/imports the neighbor); false when the neighbor
    /// points back (the neighbor calls/imports the explained node).
    pub outgoing: bool,
    pub relation: String,
    pub confidence: String,
    pub strength: f64,
    /// The stored numeric score when one exists (Jev keep-probability on
    /// verified semantic edges); None means the label rank is all there is.
    pub confidence_score: Option<f64>,
}

pub struct ExplainResult {
    pub id: String,
    pub label: String,
    pub source_file: String,
    pub source_line: Option<i64>,
    pub community: Option<i64>,
    pub neighbor_count: usize,
    pub neighbors: Vec<EdgeInfoResult>,
    /// Labels of hyperedges whose member list contains this node.
    pub hyperedges: Vec<String>,
}

/// Mermaid call-flow diagram: breadth-first over `calls` edges from one
/// seed node. Direction "out" renders what the node calls, "in" renders
/// what calls it, "both" renders the union. Output is a `flowchart LR`
/// block - GitHub, Obsidian, and mermaid.js render it natively.
pub fn callflow_mermaid(
    db: &Connection,
    db_path: &str,
    seed_query: &str,
    depth: usize,
    direction: &str,
) -> astria_core::Result<String> {
    let _transaction = read_snapshot(db)?;
    let loaded = load_graph(db, db_path)?;
    if loaded.graph.node_count() == 0 {
        return Ok("No nodes in graph.".to_string());
    }

    // Seed resolution mirrors affected/explain: exact id wins, a stub never
    // shadows a same-named definition, fuzzy scoring as the last resort.
    let resolve = |q: &str| -> Option<NodeIndex> {
        if let Some(&idx) = loaded.id_to_idx.get(q) {
            let is_stub: bool = db
                .query_row(
                    "SELECT file_type = 'stub' FROM nodes WHERE id = ?1",
                    rusqlite::params![q],
                    |r| r.get(0),
                )
                .unwrap_or(false);
            if !is_stub {
                return Some(idx);
            }
            let bare = q
                .trim_start_matches('.')
                .trim_end_matches("()")
                .to_lowercase();
            if let Some(id) = astria_core::db::prefer_non_stub_id(db, &bare) {
                if let Some(&better) = loaded.id_to_idx.get(id.as_str()) {
                    return Some(better);
                }
            }
            return Some(idx);
        }
        let terms: Vec<String> = q.split_whitespace().map(|s| s.to_string()).collect();
        score_nodes(&loaded, &terms).ranked.first().map(|(_, i)| *i)
    };

    let seed = resolve(seed_query).ok_or_else(|| {
        astria_core::AstriaError::Graph(format!("node not found: '{seed_query}'"))
    })?;

    let outgoing = direction != "in";
    let incoming = direction != "out";

    let mut seen: HashSet<NodeIndex> = HashSet::new();
    seen.insert(seed);
    let mut frontier: Vec<NodeIndex> = vec![seed];
    let mut distance: HashMap<NodeIndex, u32> = [(seed, 0)].into_iter().collect();
    let mut call_edges: Vec<(NodeIndex, NodeIndex)> = Vec::new();

    for level in 0..depth {
        let mut next: Vec<NodeIndex> = Vec::new();
        for &n in &frontier {
            let mut consider = |other: NodeIndex| {
                if !seen.contains(&other) {
                    seen.insert(other);
                    distance.insert(other, level as u32 + 1);
                    next.push(other);
                }
            };
            if outgoing {
                for e in loaded
                    .graph
                    .edges_directed(n, petgraph::Direction::Outgoing)
                {
                    if e.weight().relation == "calls" {
                        let t = e.target();
                        call_edges.push((n, t));
                        consider(t);
                    }
                }
            }
            if incoming {
                for e in loaded
                    .graph
                    .edges_directed(n, petgraph::Direction::Incoming)
                {
                    if e.weight().relation == "calls" {
                        let src = e.source();
                        call_edges.push((src, n));
                        consider(src);
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }

    // Stable render order: hop distance from the seed, then label, then id.
    let mut nodes: Vec<(u32, String, String)> = seen
        .iter()
        .map(|idx| {
            (
                distance.get(idx).copied().unwrap_or(u32::MAX),
                loaded.graph[*idx].label.clone(),
                loaded.graph[*idx].id.clone(),
            )
        })
        .collect();
    nodes.sort();

    let mermaid_id: HashMap<String, String> = nodes
        .iter()
        .enumerate()
        .map(|(i, (_, _, id))| (id.clone(), format!("n{i}")))
        .collect();

    let mut out = String::from(
        "flowchart LR
",
    );
    for (_, label, id) in &nodes {
        let short: String = label.chars().take(60).collect();
        let escaped = short.replace('"', "'");
        out.push_str(&format!(
            "  {}[\"{}\"]
",
            mermaid_id[id], escaped
        ));
    }

    let mut rendered: Vec<(String, String)> = call_edges
        .iter()
        .filter_map(|(a, b)| {
            let sa = loaded.graph[*a].id.clone();
            let sb = loaded.graph[*b].id.clone();
            let ma = mermaid_id.get(&sa)?;
            let mb = mermaid_id.get(&sb)?;
            Some((ma.clone(), mb.clone()))
        })
        .collect();
    rendered.sort();
    rendered.dedup();
    for (a, b) in &rendered {
        out.push_str(&format!(
            "  {a} --> {b}
"
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    #[test]
    fn doc_seeds_do_not_displace_code() {
        // Eight document nodes keyword-match the question; the seed quota
        // keeps the code symbol present without hiding visited records.
        let db = open_db_in_memory().unwrap();
        let mut inserts = String::from(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('sym', 'validation()', 'code', 'src/v.rs')",
        );
        for i in 0..8 {
            inserts.push_str(&format!(
                ", ('d{i}', 'validation notes {i}', 'document', 'docs/n{i}.md')"
            ));
        }
        inserts.push(';');
        inserts.push_str(
            "INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('sym', 'd0', 'references', 'EXTRACTED', 'src/v.rs');",
        );
        db.execute_batch(&inserts).unwrap();

        let (text, _, _, _) = query_graph(
            &db,
            "cap-test",
            "validation",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        let doc_lines = text
            .lines()
            .filter(|l| l.starts_with("NODE ") && l.contains("docs/"))
            .count();
        assert!(
            doc_lines <= 2,
            "doc seed quota exceeded: {doc_lines} doc nodes shown"
        );
        assert!(
            text.contains("validation()"),
            "code symbol must remain present"
        );
    }

    #[test]
    fn prefer_files_ranks_file_node_over_equal_symbol() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('anchor', 'unrelated()', 'code', 'src/anchor.rs'),
                ('fnode', 'ingest.rs', 'code', 'src/ingest.rs'),
                ('sym', 'helper()', 'code', 'src/other.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('anchor', 'fnode', 'imports', 'EXTRACTED', 'src/anchor.rs'),
                ('anchor', 'sym', 'imports', 'EXTRACTED', 'src/anchor.rs');",
        )
        .unwrap();

        // the anchor node matches the question; both discovered nodes tie at
        // zero relevance, so the file preference decides which surfaces first
        // ("unrelated" label + "anchor" id give the seed majority evidence —
        // the no-confident-match floor only refuses questions whose terms
        // the graph cannot see at all)
        let (text_files, _, _, _) = query_graph(
            &db,
            "pf-files",
            "unrelated anchor reference",
            "bfs",
            1,
            4000,
            false,
            0.0,
            0,
            true,
        )
        .unwrap();
        let first_files = text_files
            .lines()
            .filter_map(|l| l.strip_prefix("NODE "))
            .find(|l| l.contains("ingest.rs") || l.contains("helper()"))
            .unwrap();
        assert!(
            first_files.contains("ingest.rs"),
            "file node must rank first with prefer_files: {first_files}"
        );

        // without the preference the symbol wins the deterministic tiebreak
        let (plain, _, _, _) = query_graph(
            &db,
            "pf-plain",
            "unrelated anchor reference",
            "bfs",
            1,
            4000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        let first_plain = plain
            .lines()
            .filter_map(|l| l.strip_prefix("NODE "))
            .find(|l| l.contains("ingest.rs") || l.contains("helper()"))
            .unwrap();
        assert!(
            first_plain.contains("helper()"),
            "plain ordering changed: {first_plain}"
        );
    }

    #[test]
    fn callflow_mermaid_renders_calls_subgraph() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a', 'call_tool()', 'code', 'mcp/lib.rs'),
                ('b', 'graph_stats_tool_works()', 'code', 'mcp/lib.rs'),
                ('c', 'unrelated()', 'code', 'other/x.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('a', 'b', 'calls', 'EXTRACTED', 'mcp/lib.rs'),
                ('c', 'a', 'imports', 'EXTRACTED', 'other/x.rs');",
        )
        .unwrap();

        let out = callflow_mermaid(&db, "cf-test", "call_tool()", 2, "out").unwrap();
        assert!(out.contains("flowchart LR"));
        assert!(out.contains("call_tool()"));
        assert!(out.contains("graph_stats_tool_works()"));
        assert!(out.contains("-->"));
        // calls-only: an `imports` edge to an unrelated node must not render
        assert!(!out.contains("unrelated()"));

        // direction "in" renders callers, and the stub-shadowing rule holds
        let db2 = open_db_in_memory().unwrap();
        db2.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('validate_url', 'validate_url', 'stub', 'wiki.md'),
                ('src_lib::validate_url', 'validate_url()', 'code', 'ing.rs'),
                ('ing', 'ingest_url()', 'code', 'ing.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('ing', 'src_lib::validate_url', 'calls', 'EXTRACTED', 'ing.rs');",
        )
        .unwrap();
        let out2 = callflow_mermaid(&db2, "cf-in", "validate_url", 2, "in").unwrap();
        assert!(out2.contains("ingest_url()"), "caller must render: {out2}");
        assert!(
            !out2.contains("wiki.md"),
            "stub must not shadow the definition: {out2}"
        );
    }

    #[test]
    fn doc_seed_quota_lets_code_enter_the_traversal() {
        // Eight documents keyword-match the question; without the seed quota
        // all five seeds were documents, traversal never left prose, and the
        // implementing code file was unreachable.
        let db = open_db_in_memory().unwrap();
        let mut inserts = String::from(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('sym', 'validation()', 'code', 'src/v.rs')",
        );
        for i in 0..8 {
            inserts.push_str(&format!(
                ", ('d{i}', 'validation notes {i}', 'document', 'docs/n{i}.md')"
            ));
        }
        inserts.push(';');
        db.execute_batch(&inserts).unwrap();

        let (text, nodes, _, _) = query_graph(
            &db,
            "seed-quota-test",
            "validation",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        assert!(nodes > 0);
        // at least one of the top five seeds must be the code symbol
        assert!(
            text.contains("validation()"),
            "code symbol must be seeded despite doc competition"
        );
        let doc_seeds = text
            .lines()
            .next()
            .map(|l| l.matches("validation notes").count())
            .unwrap_or(0);
        assert!(doc_seeds <= 2, "doc seeds exceeded the quota: {doc_seeds}");
    }

    #[test]
    fn doc_only_graph_seeds_up_to_the_limit() {
        // On a transcript-style graph (no code at all) the two-seed prose
        // quota would leave the other seed slots empty; the quota opens up
        // when the graph is effectively doc-only.
        let db = open_db_in_memory().unwrap();
        let mut inserts = String::from(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('d0', 'sunrise session', 'document', 't/s0.md', 'Melanie painted a sunrise at the lake')",
        );
        for i in 1..6 {
            inserts.push_str(&format!(
                ", ('d{i}', 'session {i}', 'document', 't/s{i}.md', 'another sunrise story {i}')"
            ));
        }
        inserts.push(';');
        db.execute_batch(&inserts).unwrap();

        let (text, _, _, _) = query_graph(
            &db,
            "doc-only-quota",
            "sunrise",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        let doc_seeds = text
            .lines()
            .next()
            .map(|l| l.matches("session").count() + l.matches("sunrise").count())
            .unwrap_or(0);
        assert!(
            doc_seeds >= 4,
            "doc-only graph seeded only {doc_seeds} nodes, quota did not open up"
        );
    }
    #[test]
    fn chunk_bodies_cite_their_span() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, source_line, docstring) VALUES
                ('c0', 'sunrise turn', 'chunk', 't/s01.md', 14, 'Melanie painted a sunrise
and the morning felt bright
across the whole valley
with friends around
at the lake house');",
        )
        .unwrap();
        let (text, _, _, _) = query_graph(
            &db,
            "chunk-span",
            "sunrise",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        assert!(
            text.contains("span: L14-L18"),
            "chunk record must cite its covered lines: {text}"
        );
    }

    #[test]
    fn code_majority_graph_ranks_chunks_under_code() {
        // The chunk's body matches the question, but on a code corpus it must
        // not displace the exact code symbol from the top of the answer.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('sym', 'validation()', 'code', 'src/v.rs', NULL),
                ('callers', 'run_checks()', 'code', 'src/w.rs', NULL),
                ('c0', 'notes turn', 'chunk', 'docs/n.md', 'validation notes about prose'),
                ('c1', 'other turn', 'chunk', 'docs/m.md', 'unrelated text'),
                ('doc0', 'validation notes', 'document', 'docs/a.md', NULL);",
        )
        .unwrap();
        let (text, _, _, _) = query_graph(
            &db,
            "chunk-prior",
            "validation",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        let first_node = text.lines().find(|l| l.starts_with("NODE ")).unwrap();
        assert!(
            first_node.contains("validation()"),
            "code symbol must outrank chunked prose on a code corpus: {first_node}"
        );
        assert!(
            text.contains("notes turn"),
            "the chunk must still surface, just ranked under code"
        );
    }

    #[test]
    fn label_lucky_chunk_does_not_outrank_body_evidence() {
        // The chunk whose body text actually answers all four terms (the
        // evidence, like the session where an event happened) must rank
        // above a later chunk whose truncated first line happens to repeat
        // the question's phrasing but whose body is unrelated.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('e0', 'trip recap', 'chunk', 't/s0.md', 'Melanie painted a sunrise at the lake with friends around'),
                ('d0', 'Melanie painted a sunrise at the lake', 'chunk', 't/s1.md', 'we talked about our trip and what happened on the beach');",
        )
        .unwrap();
        let (text, _, _, _) = query_graph(
            &db,
            "chunk-label-luck",
            "Melanie painted a sunrise at the lake",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        let first_node = text.lines().find(|l| l.starts_with("NODE ")).unwrap();
        assert!(
            first_node.contains("t/s0.md"),
            "body evidence must outrank label luck: {first_node}"
        );
    }

    #[test]
    fn embed_optout_env_values() {
        assert!(!semantic_seeds_disabled_value(None));
        assert!(!semantic_seeds_disabled_value(Some("on")));
        assert!(!semantic_seeds_disabled_value(Some("1")));
        assert!(!semantic_seeds_disabled_value(Some("")));
        assert!(!semantic_seeds_disabled_value(Some("auto")));
        assert!(semantic_seeds_disabled_value(Some("off")));
        assert!(semantic_seeds_disabled_value(Some("OFF")));
        assert!(semantic_seeds_disabled_value(Some(" false ")));
        assert!(semantic_seeds_disabled_value(Some("no")));
        assert!(semantic_seeds_disabled_value(Some("0")));
    }

    #[test]
    fn stopwords_neutralized_and_code_ranks_over_document() {
        // "where does validation happen": question words must score nothing,
        // and on equal term evidence ("validation") the code symbol outranks
        // a prose document node.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('doc', 'validation notes', 'document', 'docs/notes.md'),
                ('sym', 'validation()', 'code', 'src/ingest.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('sym', 'doc', 'references', 'EXTRACTED', 'src/ingest.rs');",
        )
        .unwrap();

        let loaded = load_graph_snapshot(&db, "scorer-prior-test").unwrap();
        let scored = score_nodes(&loaded, &["validation".to_string()]).ranked;
        assert!(scored.len() >= 2);
        // equal term evidence (both labels contain "validation") — the code
        // symbol must outrank the prose node
        assert_eq!(loaded.graph[scored[0].1].file_type, "code");

        // question words alone match nothing
        let empty = score_nodes(&loaded, &["where".to_string(), "does".to_string()]);
        assert!(empty.ranked.is_empty());
    }

    #[test]
    fn ubiquitous_terms_downweighted_so_rare_targets_seed() {
        // "scip index ingestion": "index" matches every index.* file in the
        // repo while "scip" names exactly one — the rare term must win the
        // seed race, not lose it to alphabetical tie-breaking among the
        // generic matches.
        let db = open_db_in_memory().unwrap();
        let mut inserts = String::from(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('scip', 'scip.rs', 'code', 'crates/astria-ingest/src/scip.rs')",
        );
        for (id, label, path) in [
            ("i1", "index.js", "website/src/pages/index.js"),
            ("i2", "index.ts", "packages/cli/src/index.ts"),
            ("i3", "index.module.css", "web/src/index.module.css"),
            ("i4", "index.tsx", "app/src/index.tsx"),
            ("i5", "index.php", "legacy/public/index.php"),
        ] {
            inserts.push_str(&format!(", ('{id}', '{label}', 'code', '{path}')"));
        }
        inserts.push(';');
        db.execute_batch(&inserts).unwrap();

        let loaded = load_graph_snapshot(&db, "idf-test").unwrap();
        let scored = score_nodes(
            &loaded,
            &[
                "scip".to_string(),
                "index".to_string(),
                "ingestion".to_string(),
            ],
        )
        .ranked;
        assert!(!scored.is_empty());
        assert_eq!(
            loaded.graph[scored[0].1].label, "scip.rs",
            "the rare-term target must outrank ubiquitous 'index' matches"
        );
    }

    #[test]
    fn entry_point_intent_boosts_import_root_file() {
        // "what is the CLI entry point": no file is named "entry", so lexical
        // scoring ties everything on a path token ("cli"); the import DAG
        // knows the answer — a file importing many commands and imported by
        // none is the program's front door.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('entry', 'index.ts', 'code', 'packages/cli/src/index.ts'),
                ('noise', 'cli.test.ts', 'code', 'packages/cli/src/__tests__/cli.test.ts'),
                ('doc', 'CLI reference', 'reference', 'website/docs/cli.md'),
                ('cmd1', 'run.ts', 'code', 'packages/cli/src/commands/run.ts'),
                ('cmd2', 'query.ts', 'code', 'packages/cli/src/commands/query.ts'),
                ('cmd3', 'map.ts', 'code', 'packages/cli/src/commands/map.ts'),
                ('cmd4', 'export.ts', 'code', 'packages/cli/src/commands/export.ts');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('entry', 'cmd1', 'imports', 'EXTRACTED', 'packages/cli/src/index.ts'),
                ('entry', 'cmd2', 'imports', 'EXTRACTED', 'packages/cli/src/index.ts'),
                ('entry', 'cmd3', 'imports', 'EXTRACTED', 'packages/cli/src/index.ts'),
                ('entry', 'cmd4', 'imports', 'EXTRACTED', 'packages/cli/src/index.ts');",
        )
        .unwrap();

        let loaded = load_graph_snapshot(&db, "entry-intent-test").unwrap();
        let scored = score_nodes(
            &loaded,
            &[
                "what".to_string(),
                "is".to_string(),
                "the".to_string(),
                "CLI".to_string(),
                "entry".to_string(),
                "point".to_string(),
            ],
        )
        .ranked;
        assert!(!scored.is_empty());
        let top: Vec<&str> = scored
            .iter()
            .take(3)
            .map(|(_, idx)| loaded.graph[*idx].id.as_str())
            .collect();
        assert!(
            top.contains(&"entry"),
            "the import root must seed on entry-point intent; top3 = {top:?}"
        );
    }

    #[test]
    fn entry_intent_requires_an_intact_phrase() {
        // "domain files" contains the substring "main file"; the intent must
        // not fire on it. A false intent boosts the import root above every
        // lexical match the question earned.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('entry', 'index.ts', 'code', 'packages/cli/src/index.ts'),
                ('noise', 'cli.test.ts', 'code', 'packages/cli/src/__tests__/cli.test.ts'),
                ('cmd1', 'run.ts', 'code', 'packages/cli/src/commands/run.ts'),
                ('cmd2', 'query.ts', 'code', 'packages/cli/src/commands/query.ts'),
                ('cmd3', 'map.ts', 'code', 'packages/cli/src/commands/map.ts'),
                ('cmd4', 'export.ts', 'code', 'packages/cli/src/commands/export.ts');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('entry', 'cmd1', 'imports', 'EXTRACTED', 'packages/cli/src/index.ts'),
                ('entry', 'cmd2', 'imports', 'EXTRACTED', 'packages/cli/src/index.ts'),
                ('entry', 'cmd3', 'imports', 'EXTRACTED', 'packages/cli/src/index.ts'),
                ('entry', 'cmd4', 'imports', 'EXTRACTED', 'packages/cli/src/index.ts');",
        )
        .unwrap();

        let loaded = load_graph_snapshot(&db, "entry-substring-test").unwrap();
        let scored = score_nodes(
            &loaded,
            &["how", "does", "query", "handle", "domain", "files"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        )
        .ranked;
        // "query" genuinely matches cmd2 — the ranked list is non-empty...
        assert!(
            scored
                .iter()
                .any(|(_, idx)| loaded.graph[*idx].id == "cmd2"),
            "the lexically matching command must be ranked: {:?}",
            scored
                .iter()
                .take(3)
                .map(|(_, i)| loaded.graph[*i].id.clone())
                .collect::<Vec<_>>()
        );
        // ...but the import root must not be boosted into it by a substring.
        assert!(
            scored
                .iter()
                .all(|(_, idx)| loaded.graph[*idx].id != "entry"),
            "'main file' as a substring of 'domain files' must not fire entry intent: {:?}",
            scored
                .iter()
                .take(3)
                .map(|(_, i)| loaded.graph[*i].id.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn bootstrap_word_alone_does_not_fire_entry_intent() {
        // "bootstrap" names a CSS framework in plenty of repos; as a bare
        // word it must not re-rank import roots above the lexically
        // matching nodes. (It was removed from the phrase list for exactly
        // this failure mode.)
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('entry', 'index.ts', 'code', 'src/index.ts'),
                ('boot', 'bootstrap.scss', 'code', 'web/static/bootstrap.scss'),
                ('cmd1', 'run.ts', 'code', 'src/commands/run.ts'),
                ('cmd2', 'query.ts', 'code', 'src/commands/query.ts'),
                ('cmd3', 'map.ts', 'code', 'src/commands/map.ts');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('entry', 'cmd1', 'imports', 'EXTRACTED', 'src/index.ts'),
                ('entry', 'cmd2', 'imports', 'EXTRACTED', 'src/index.ts'),
                ('entry', 'cmd3', 'imports', 'EXTRACTED', 'src/index.ts');",
        )
        .unwrap();

        let loaded = load_graph_snapshot(&db, "entry-bootstrap-test").unwrap();
        let scored = score_nodes(
            &loaded,
            &["how", "does", "the", "app", "load", "bootstrap", "styles"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        )
        .ranked;
        assert!(
            !scored.is_empty(),
            "the stylesheet node matches 'bootstrap' lexically"
        );
        assert_eq!(
            loaded.graph[scored[0].1].id,
            "boot",
            "the lexically matching stylesheet must outrank everything: {:?}",
            scored
                .iter()
                .take(3)
                .map(|(_, i)| loaded.graph[*i].id.clone())
                .collect::<Vec<_>>()
        );
        assert!(
            scored
                .iter()
                .all(|(_, idx)| loaded.graph[*idx].id != "entry"),
            "bare 'bootstrap' must not fire entry intent"
        );
    }

    #[test]
    fn corpus_mode_auto_detects_and_pins() {
        use CorpusMode::{CodeMajority, DocsMajority};
        // auto-detection at the designed threshold
        assert_eq!(corpus_mode_from(0.94, None), CodeMajority);
        assert_eq!(corpus_mode_from(0.95, None), DocsMajority);
        assert_eq!(corpus_mode_from(1.00, None), DocsMajority);
        // the pin overrides the share in both directions
        assert_eq!(corpus_mode_from(1.00, Some(CodeMajority)), CodeMajority);
        assert_eq!(corpus_mode_from(0.10, Some(DocsMajority)), DocsMajority);
        // recognized pin values, case- and padding-insensitive
        assert_eq!(corpus_mode_pin_value("docs"), Some(DocsMajority));
        assert_eq!(corpus_mode_pin_value(" CODE "), Some(CodeMajority));
        assert_eq!(corpus_mode_pin_value("bogus"), None);
    }

    #[test]
    fn semantic_seeds_match_without_string_overlap() {
        // "auth flow" shares no tokens with "SessionMiddleware" — token
        // scoring finds nothing; a semantic candidate must seed the traversal.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('m', 'SessionMiddleware', 'code', 'src/session.rs'),
                ('v', 'validateSession()', 'code', 'src/session.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('m', 'v', 'defines', 'EXTRACTED', 'src/session.rs');",
        )
        .unwrap();

        // token-only: no match (unique db_path — the graph cache is keyed
        // by db_path, and ":memory:" would collide across tests)
        let (text, nodes, _, _) = query_graph(
            &db,
            "mem-semantic-test",
            "auth flow",
            "bfs",
            2,
            500,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        assert!(
            text.contains("No matching nodes"),
            "token-only should miss: {text}"
        );
        assert_eq!(nodes, 0);

        // with a semantic candidate the traversal runs
        let semantic = vec![("m".to_string(), 0.9), ("v".to_string(), 0.7)];
        let (text, nodes, _, _) = query_graph_with_semantic(
            &db,
            "mem-semantic-test",
            "auth flow",
            "bfs",
            2,
            500,
            false,
            0.0,
            0,
            &semantic,
            false,
        )
        .unwrap();
        assert!(
            text.contains("SessionMiddleware"),
            "semantic seed should drive traversal: {text}"
        );
        assert!(nodes > 0);
    }

    #[test]
    fn semantic_score_rescales_below_exact_label_match() {
        // a perfect cosine (1.0) caps at the calibration cap — under an exact label hit
        let c = &astria_core::calibration::SEMANTIC_CALIBRATION;
        assert!(semantic_seed_score(1.0) < 1.0);
        assert!((semantic_seed_score(1.0) - c.seed_score_cap).abs() < 1e-9);
        // at or below the measured noise floor it contributes nothing
        assert_eq!(semantic_seed_score(c.noise_floor), 0.0);
        assert_eq!(semantic_seed_score(c.noise_floor - 0.15), 0.0);
        // between the anchors it is positive and monotone
        assert!(semantic_seed_score((c.noise_floor + c.strong_match) / 2.0) > 0.0);
        assert!(semantic_seed_score(c.noise_floor + 0.05) < semantic_seed_score(c.strong_match));
    }

    #[test]
    fn queries_record_pairs_and_promotion_gates_on_distinct_questions() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('auth', 'AuthService', 'code', 'src/auth.rs'),
                ('sess', 'SessionManager', 'code', 'src/sess.rs'),
                ('tok', 'TokenValidator', 'code', 'src/tok.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('auth', 'sess', 'calls', 'EXTRACTED', 'src/auth.rs'),
                ('sess', 'tok', 'calls', 'EXTRACTED', 'src/sess.rs');",
        )
        .unwrap();

        // two distinct questions whose traversals connect the same pair
        // (both must token-match: unmatched questions return early)
        for question in ["session handling", "session validation"] {
            query_graph(
                &db,
                "mem-pairs-test",
                question,
                "bfs",
                2,
                500,
                false,
                0.0,
                0,
                false,
            )
            .unwrap();
        }

        let recorded: i64 = db
            .query_row("SELECT COUNT(*) FROM query_pairs", [], |r| r.get(0))
            .unwrap();
        assert!(recorded > 0, "queries should record seed/discovery pairs");

        // threshold not met yet: 2 distinct questions but each hit once
        let promoted = promote_learned_edges(&db, 2, 3).unwrap();
        assert_eq!(promoted, 0, "needs min_hits across distinct questions");

        // one more repeat pushes hits over the bar
        query_graph(
            &db,
            "mem-pairs-test",
            "session validation",
            "bfs",
            2,
            500,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        let promoted = promote_learned_edges(&db, 2, 3).unwrap();
        assert!(
            promoted > 0,
            "recurring pairs across questions should promote"
        );

        let (relation, confidence, score, source): (String, String, f64, String) = db
            .query_row(
                "SELECT relation, confidence, confidence_score, source_file
                 FROM edges WHERE relation = 'learned' LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(relation, "learned");
        assert_eq!(confidence, "INFERRED");
        assert_eq!(source, "query_history");
        assert!(score > 0.0 && score <= 1.0);

        // promotion is idempotent — regenerate without duplicating
        let again = promote_learned_edges(&db, 2, 3).unwrap();
        assert_eq!(again, promoted);
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE relation = 'learned'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count as usize, promoted);
    }

    #[test]
    fn promotion_skips_pairs_whose_nodes_were_deleted() {
        // query_pairs rows can outlive their nodes (files removed later) —
        // promotion must drop them, not die on the edges foreign key.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a', 'Alpha', 'code', 'a.rs'),
                ('b', 'Beta', 'code', 'b.rs');
            INSERT INTO query_pairs (source, target, question, hits, first_seen, last_seen) VALUES
                ('a', 'b', 'q1', 2, '1', '1'),
                ('a', 'b', 'q2', 2, '1', '1'),
                ('a', 'ghost', 'q1', 5, '1', '1'),
                ('ghost', 'b', 'q2', 5, '1', '1');",
        )
        .unwrap();

        let promoted = promote_learned_edges(&db, 2, 3).unwrap();
        assert_eq!(promoted, 1, "only the (a,b) pair qualifies and inserts");
        let stale: i64 = db
            .query_row("SELECT COUNT(*) FROM query_pairs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stale, 2, "orphaned pairs are cleaned up");
    }

    fn seed(db: &Connection) -> String {
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a', 'Alpha', 'code', 'src/alpha.rs'),
                ('b', 'BetaService', 'code', 'src/beta.rs'),
                ('c', 'Gamma', 'code', 'src/gamma.rs'),
                ('d', 'Delta', 'code', 'src/delta.rs');
            INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file) VALUES
                ('a', 'b', 'calls', 'EXTRACTED', 1.0, 'src/alpha.rs'),
                ('b', 'c', 'calls', 'SEMANTIC', NULL, 'src/beta.rs'),
                ('c', 'd', 'imports', 'INFERRED', 0.7, 'src/gamma.rs');",
        )
        .unwrap();
        "test".to_string()
    }

    fn loaded(db: &Connection, key: &str) -> LoadedGraph {
        load_graph_snapshot(db, key).unwrap()
    }

    #[test]
    fn tokenize_splits_case_and_separators() {
        assert_eq!(
            tokenize("parseExtractionText"),
            vec!["parse", "extraction", "text"]
        );
        assert_eq!(
            tokenize("parse_extraction_text"),
            vec!["parse", "extraction", "text"]
        );
        assert_eq!(
            tokenize("Parse-Extraction.Text"),
            vec!["parse", "extraction", "text"]
        );
    }

    #[test]
    fn camel_query_finds_snake_label() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('n', 'parse_extraction_text', 'code', 'src/p.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('n', 'n', 'calls', 'EXTRACTED', 'src/p.rs');",
        )
        .unwrap();
        let g = loaded(&db, "camel");
        let scored = score_nodes(&g, &["parseExtractionText".to_string()]);
        assert_eq!(
            scored.ranked.len(),
            1,
            "camelCase query should match snake_case label"
        );
    }

    #[test]
    fn salient_rare_terms_beat_many_common_matches() {
        // RepoQA dilution shape: one node matches the query's rare terms,
        // a doc-body node weakly matches many common ones. The unnormalized
        // sum used to bury the rare-matcher; salient coverage restores it.
        let db = open_db_in_memory().unwrap();
        let mut diluter_body = String::new();
        for i in 0..20 {
            diluter_body.push_str(&format!(
                "Line {} of the general output input list data. ",
                i % 5 + 1
            ));
        }
        db.execute_batch(&format!(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('target', 'merge_string_group', 'code', 'src/t.rs', 'Merges zanzibar adjacent fragment groups.'),
                ('diluter', 'notes', 'document', 'docs/notes.md', '{diluter_body}'),
                ('f1', 'alpha_one', 'code', 'src/a.rs', NULL),
                ('f2', 'beta_two', 'code', 'src/b.rs', NULL),
                ('f3', 'gamma_three', 'code', 'src/g.rs', NULL),
                ('f4', 'delta_four', 'code', 'src/d.rs', NULL);"
        ))
        .unwrap();
        let g = loaded(&db, "salient");
        // zanzibar/fragment/adjacent appear only in the target's docstring;
        // line/output/input/list/data saturate the diluter's body.
        let terms: Vec<String> = "zanzibar adjacent fragment general output input list data"
            .split_whitespace()
            .map(String::from)
            .collect();
        let scored = score_nodes(&g, &terms);
        assert!(!scored.ranked.is_empty());
        assert_eq!(
            scored.ranked[0].1, g.id_to_idx["target"],
            "rare-term matcher must outrank the common-term diluter; got {:?}",
            g.graph[scored.ranked[0].1].label
        );
    }

    #[test]
    fn semantic_only_candidate_reserves_a_seed_slot() {
        // Zero string overlap: the question matches only the filler nodes;
        // the target carries embedding evidence (cosine 0.9) that no query
        // term touches. The reservation must start the traversal there.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('filler_1', 'delta_one', 'code', 'src/1.rs', NULL),
                ('filler_2', 'gamma_two', 'code', 'src/2.rs', NULL),
                ('filler_3', 'four_three', 'code', 'src/3.rs', NULL),
                ('filler_4', 'delta_five', 'code', 'src/4.rs', NULL),
                ('filler_5', 'gamma_six', 'code', 'src/5.rs', NULL),
                ('filler_6', 'four_seven', 'code', 'src/6.rs', NULL),
                ('target', 'maybe_install_helper', 'code', 'src/h.rs', 'uses an alternative technology for speed');",
        )
        .unwrap();
        let g = loaded(&db, "semseed");
        let (text, _, _, _) = query_graph_loaded(
            &db,
            g,
            "delta gamma four",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            &[("target".to_string(), 0.9)],
            false,
            None,
        )
        .unwrap();
        assert!(
            text.contains("maybe_install_helper"),
            "semantic-only candidate must be seeded and traversed; output:\n{text}"
        );
    }

    #[test]
    fn semantic_only_reservation_requires_meaningful_similarity() {
        // Below the floor (cosine 0.5 rescales to 0), a distant node must
        // NOT displace token-matched seeds or claim the reserved slot.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('filler_1', 'delta_one', 'code', 'src/1.rs', NULL),
                ('filler_2', 'gamma_two', 'code', 'src/2.rs', NULL),
                ('filler_3', 'four_three', 'code', 'src/3.rs', NULL),
                ('filler_4', 'delta_five', 'code', 'src/4.rs', NULL),
                ('filler_5', 'gamma_six', 'code', 'src/5.rs', NULL),
                ('filler_6', 'four_seven', 'code', 'src/6.rs', NULL),
                ('target', 'maybe_install_helper', 'code', 'src/h.rs', 'uses an alternative technology for speed');",
        )
        .unwrap();
        let g = loaded(&db, "semseed-floor");
        let (text, _, _, _) = query_graph_loaded(
            &db,
            g,
            "delta gamma four",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            &[("target".to_string(), 0.5)],
            false,
            None,
        )
        .unwrap();
        assert!(
            !text.contains("maybe_install_helper"),
            "below-floor semantic candidate must stay out; output:\n{text}"
        );
    }

    #[test]
    fn semantic_floor_blocks_only_weak_semantic_edges() {
        let db = open_db_in_memory().unwrap();
        let key = seed(&db);
        let g = loaded(&db, &key);
        let start = g.id_to_idx["a"];
        // seed(): a -EXTRACTED-> b -SEMANTIC(no score)-> c -INFERRED-> d
        let (visited_open, _, _) = bfs_subgraph(&g, &[start], 3, true, 0.0, 0.0);
        assert_eq!(visited_open.len(), 4, "floor off: every edge is crossed");
        let (visited_floored, _, _) = bfs_subgraph(&g, &[start], 3, true, 0.0, 0.8);
        assert_eq!(
            visited_floored.len(),
            2,
            "floor 0.8: the unscored SEMANTIC edge (label rank 0.6) is blocked, EXTRACTED is not"
        );
    }

    #[test]
    fn weakly_reached_nodes_sort_behind_strongly_reached() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('w', 'Widget', 'code', 'src/w.rs'),
                ('s', 'Sprocket', 'code', 'src/s.rs');",
        )
        .unwrap();
        let g = loaded(&db, "weaksort");
        let w = g.id_to_idx["w"];
        let s = g.id_to_idx["s"];
        let mut visited = HashSet::new();
        visited.insert(w);
        visited.insert(s);
        let relevance = [(w, 1.0), (s, 1.0)].into_iter().collect();
        let distance = [(w, 1u32), (s, 1u32)].into_iter().collect();
        // w's best touching edge is a below-floor SEMANTIC link; s sits on
        // structural evidence. Equal relevance and distance: w must yield.
        let reach: HashMap<NodeIndex, f64> = [(w, 0.5), (s, 0.9)].into_iter().collect();
        let (text, _) = subgraph_to_text(
            &g,
            &visited,
            &[],
            &relevance,
            &distance,
            &reach,
            false,
            10_000,
            0,
            "",
        )
        .unwrap();
        let s_pos = text.find("Sprocket").unwrap();
        let w_pos = text.find("Widget").unwrap();
        assert!(
            s_pos < w_pos,
            "weakly-reached node must sort last:
{text}"
        );
    }

    #[test]
    fn below_semantic_floor_unit() {
        let edge = |conf: &str, score: Option<f64>| EdgeData {
            relation: "relates_to".into(),
            confidence: conf.into(),
            confidence_score: score,
            source_file: String::new(),
            source_line: None,
        };
        assert!(below_semantic_floor(&edge("SEMANTIC", Some(0.5)), 0.65));
        assert!(!below_semantic_floor(&edge("SEMANTIC", Some(0.9)), 0.65));
        assert!(
            below_semantic_floor(&edge("SEMANTIC", None), 0.65),
            "unscored falls back to label rank 0.6, below the floor"
        );
        assert!(
            !below_semantic_floor(&edge("INFERRED", Some(0.4)), 0.65),
            "structural edges are never floored"
        );
        assert!(
            !below_semantic_floor(&edge("SEMANTIC", Some(0.1)), 0.0),
            "0.0 disables the floor"
        );
    }

    #[test]
    fn directed_bfs_follows_edge_direction_only() {
        let db = open_db_in_memory().unwrap();
        let key = seed(&db);
        let g = loaded(&db, &key);

        let a = g.id_to_idx["a"];
        // b imports d? No — c imports d, so from 'a', directed BFS cannot
        // reach 'd' backwards through a->b<-? a->b->c->d is forward: use
        // 'd' as seed and confirm directed traversal does NOT walk
        // imports backwards to 'c'.
        let d = g.id_to_idx["d"];
        let (visited_fwd, _, _) = bfs_subgraph(&g, &[d], 2, true, 0.0, 0.0);
        assert!(!visited_fwd.contains(&g.id_to_idx["c"]));

        let (visited_und, _, _) = bfs_subgraph(&g, &[d], 2, false, 0.0, 0.0);
        assert!(visited_und.contains(&g.id_to_idx["c"]));

        let _ = a;
    }

    #[test]
    fn directed_path_respects_direction() {
        let db = open_db_in_memory().unwrap();
        let key = seed(&db);
        // a -> b -> c -> d: directed path a..d exists; d..a does not
        let (found, hops, _) = find_shortest_path(&db, &key, "Alpha", "Delta", true, 0.0).unwrap();
        assert!(found);
        assert_eq!(hops, 3);

        let (found_rev, _, _) = find_shortest_path(&db, &key, "Delta", "Alpha", true, 0.0).unwrap();
        assert!(!found_rev);

        let (found_und, hops_und, _) =
            find_shortest_path(&db, &key, "Delta", "Alpha", false, 0.0).unwrap();
        assert!(found_und);
        assert_eq!(hops_und, 3);
    }

    #[test]
    fn explain_sorts_by_numeric_strength_not_alphabet() {
        let db = open_db_in_memory().unwrap();
        let key = seed(&db);
        let result = explain_with_neighbors(&db, &key, "b").unwrap().unwrap();
        assert_eq!(result.neighbors.len(), 2);
        // b -> c is SEMANTIC (fallback 0.6), a -> b is EXTRACTED 1.0.
        // Alphabetical ("EXTRACTED" < "SEMANTIC") would put SEMANTIC first.
        assert_eq!(result.neighbors[0].neighbor_id, "a");
        assert!(result.neighbors[0].strength > result.neighbors[1].strength);
    }

    #[test]
    fn scoring_is_deterministic_on_ties() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('x', 'handler', 'code', 'src/x.rs'),
                ('y', 'handler', 'code', 'src/y.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('x', 'y', 'calls', 'EXTRACTED', 'src/x.rs');",
        )
        .unwrap();
        let g = loaded(&db, "ties");
        let s1 = score_nodes(&g, &["handler".to_string()]).ranked;
        let s2 = score_nodes(&g, &["handler".to_string()]).ranked;
        let ids = |s: &[(f64, NodeIndex)]| -> Vec<String> {
            s.iter().map(|(_, i)| g.graph[*i].id.clone()).collect()
        };
        assert_eq!(ids(&s1), ids(&s2));
    }

    #[test]
    fn confidence_rank_orders_labels() {
        assert!(confidence_rank("DECLARED") > confidence_rank("EXTRACTED"));
        assert!(confidence_rank("EXTRACTED") > confidence_rank("INFERRED"));
        assert!(confidence_rank("INFERRED") > confidence_rank("SEMANTIC"));
    }

    #[test]
    fn stale_graph_discloses_changed_files() {
        // An agent that cannot tell the graph predicates its answers on an
        // old build answers confidently from the past. A file in the
        // manifest modified after publication must be disclosed; a fresh
        // graph must stay quiet.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("app.rs");
        std::fs::write(&file, "fn a() {}\n").unwrap();
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a', 'alpha()', 'code', 'src/a.rs');
             INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('a', 'a', 'calls', 'EXTRACTED', 'src/a.rs');",
        )
        .unwrap();
        db.execute(
            "INSERT INTO file_manifest (file_path, content_hash, file_type, last_seen_at, size_bytes)
             VALUES (?1, 'x', 'code', '2026-01-01', 9)",
            rusqlite::params![file.to_string_lossy().to_string()],
        )
        .unwrap();

        // Published long ago, file written now → stale disclosure.
        db.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('graph_published_at', '1000')",
            [],
        )
        .unwrap();
        let g = load_graph_snapshot(&db, "stale-yes").unwrap();
        let (text, _, _, _) = query_graph_loaded(
            &db,
            g,
            "alpha",
            "bfs",
            1,
            2000,
            false,
            0.0,
            0,
            &[],
            false,
            Some("1000"),
        )
        .unwrap();
        assert!(
            text.contains("changed since this build"),
            "stale graph must disclose: {text}"
        );

        // Published in the future (or just now) → quiet.
        let fresh_ts = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 60)
            .to_string();
        let g2 = load_graph_snapshot(&db, "stale-no").unwrap();
        let (text2, _, _, _) = query_graph_loaded(
            &db,
            g2,
            "alpha",
            "bfs",
            1,
            2000,
            false,
            0.0,
            0,
            &[],
            false,
            Some(fresh_ts.as_str()),
        )
        .unwrap();
        assert!(
            !text2.contains("changed since this build"),
            "fresh graph must stay quiet: {text2}"
        );
    }

    #[test]
    fn no_match_suggests_nearest_labels() {
        let db = open_db_in_memory().unwrap();
        let key = seed(&db);
        // A typo can be rescued by fuzzy retrieval or nearest-label guidance.
        let (text, _, _, _) =
            query_graph(&db, &key, "Alpga", "bfs", 2, 2000, false, 0.0, 0, false).unwrap();
        assert!(
            text.contains("Alpha"),
            "typo should retain matching guidance, got: {text}"
        );

        // Gibberish with no near match: clean no-match, no suggestions.
        let (text2, nodes2, _, _) = query_graph(
            &db,
            &key,
            "zzzqqq xxxvvv",
            "bfs",
            2,
            2000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        assert_eq!(nodes2, 0);
        assert!(text2.starts_with("No matching nodes found."));

        // A label in the narrow similarity band (matched by neither exact
        // nor the fuzzy-rescue threshold) surfaces via did-you-mean.
        let g = load_graph_snapshot(&db, &key).unwrap();
        let sugg = nearest_labels(&g, "Ahpah", 3);
        assert!(
            sugg.iter().any(|s| s.contains("Alpha")),
            "nearest_labels should surface Alpha, got: {sugg:?}"
        );
    }

    #[test]
    fn seed_floor_env_values() {
        assert!(seed_floor_enabled_value(None));
        assert!(seed_floor_enabled_value(Some("on")));
        assert!(seed_floor_enabled_value(Some("1")));
        assert!(seed_floor_enabled_value(Some("")));
        assert!(!seed_floor_enabled_value(Some("off")));
        assert!(!seed_floor_enabled_value(Some("OFF")));
        assert!(!seed_floor_enabled_value(Some(" false ")));
        assert!(!seed_floor_enabled_value(Some("no")));
        assert!(!seed_floor_enabled_value(Some("0")));
    }

    #[test]
    fn incidental_word_matches_refuse_to_seed_traversal() {
        // The measured failure: "Where is OAuth authentication handled?"
        // against a graph with no auth code. The only evidence is a fuzzy
        // hit of "handled" on handle_message() — an incidental word match,
        // not an answer. The traversal must return an explicit miss instead
        // of a full budget of authoritative-looking noise.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('h', 'handle_message()', 'code', 'src/msg.rs'),
                ('s', 'sanitize_inputs()', 'code', 'src/io.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('h', 's', 'calls', 'EXTRACTED', 'src/msg.rs');",
        )
        .unwrap();
        let g = loaded(&db, "floor-refuse");
        let (text, nodes, _, _) = query_graph_loaded(
            &db,
            g,
            "Where is OAuth authentication handled?",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            &[],
            false,
            None,
        )
        .unwrap();
        assert_eq!(nodes, 0, "an incidental match must not seed a traversal");
        assert!(
            text.contains("No confident match"),
            "the miss must be explicit, got: {text}"
        );
        assert!(
            text.contains("authentication"),
            "the miss should name the key terms it could not find, got: {text}"
        );
    }

    #[test]
    fn semantic_candidate_overrides_seed_floor() {
        // Same incidental-only question, but embedding evidence (cosine
        // 0.9) points at a node no query term touches. Zero-overlap
        // conceptual queries are exactly what embeddings are for — the
        // floor must yield to a qualifying semantic candidate.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('h', 'handle_message()', 'code', 'src/msg.rs', NULL),
                ('s', 'sanitize_inputs()', 'code', 'src/io.rs', NULL);
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('h', 's', 'calls', 'EXTRACTED', 'src/msg.rs');",
        )
        .unwrap();
        let g = loaded(&db, "floor-semantic");
        let (text, nodes, _, _) = query_graph_loaded(
            &db,
            g,
            "Where is OAuth authentication handled?",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            &[("s".to_string(), 0.9)],
            false,
            None,
        )
        .unwrap();
        assert!(nodes > 0, "a semantic candidate must still traverse");
        assert!(
            text.contains("sanitize_inputs"),
            "the semantic candidate's neighborhood should be returned, got: {text}"
        );
    }

    #[test]
    fn single_common_word_does_not_rescue_an_absent_topic() {
        // The payroll probe: "How does the payroll module calculate
        // overtime pay?" against a graph with no payroll code. "module"
        // fully covers some labels — real evidence, but one common word of
        // five effective terms does not identify an answer, and the
        // identifying terms (payroll, overtime) match nothing. The floor
        // must refuse instead of traversing from index.module.css.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('css', 'index.module.css', 'code', 'web/src/index.module.css'),
                ('imp', 'extract_import_module()', 'code', 'src/walkers.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('imp', 'css', 'references', 'EXTRACTED', 'src/walkers.rs');",
        )
        .unwrap();
        let g = loaded(&db, "floor-payroll");
        let (text, nodes, _, _) = query_graph_loaded(
            &db,
            g,
            "How does the payroll module calculate overtime pay?",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            &[],
            false,
            None,
        )
        .unwrap();
        assert_eq!(nodes, 0, "a lone common-word match must not seed");
        assert!(
            text.contains("No confident match"),
            "the miss must be explicit, got: {text}"
        );
    }

    #[test]
    fn description_shaped_queries_rank_semantic_evidence_first() {
        // "What handles the graviton phase" style: the question's
        // identifying terms exist nowhere in the graph, the only lexical
        // hits are one common word, and the embedding model points at the
        // real answer with a strong calibrated cosine. There the semantic
        // evidence must OUTRANK the partial token matches (lead the
        // response), instead of capping below them and waiting for the
        // reservation slot.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('l1', 'list_one', 'code', 'src/1.rs'),
                ('l2', 'list_two', 'code', 'src/2.rs'),
                ('l3', 'list_three', 'code', 'src/3.rs'),
                ('l4', 'list_four', 'code', 'src/4.rs'),
                ('l5', 'list_five', 'code', 'src/5.rs'),
                ('target', 'maybe_install_helper', 'code', 'src/h.rs');",
        )
        .unwrap();
        let g = loaded(&db, "adaptive-semantic");
        let (text, _, _, _) = query_graph_loaded(
            &db,
            g,
            "zanzibar graviton list",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            &[("target".to_string(), 0.9)],
            false,
            None,
        )
        .unwrap();
        let first_node = text
            .lines()
            .find(|l| l.starts_with("NODE "))
            .expect("the semantic answer's neighborhood should be returned");
        assert!(
            first_node.contains("maybe_install_helper"),
            "with zero salient evidence a strong cosine must lead, got: {first_node}"
        );
    }

    #[test]
    fn salient_evidence_traverses_despite_weak_scores() {
        // A weak but on-topic match must NOT be refused: the question's key
        // term has real evidence in the graph, so the traversal runs even
        // though the scores are small. The floor only fires on zero salient
        // evidence, not on low absolute scores.
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('h', 'handle_message()', 'code', 'src/msg.rs', 'Handles the OAuth token exchange flow.');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('h', 'h', 'calls', 'EXTRACTED', 'src/msg.rs');",
        )
        .unwrap();
        let g = loaded(&db, "floor-salient");
        let (text, nodes, _, _) = query_graph_loaded(
            &db,
            g,
            "Where is OAuth authentication handled?",
            "bfs",
            2,
            4000,
            false,
            0.0,
            0,
            &[],
            false,
            None,
        )
        .unwrap();
        assert!(nodes > 0, "salient evidence must allow the traversal");
        assert!(
            text.contains("handle_message"),
            "the on-topic node should be returned, got: {text}"
        );
    }

    #[test]
    fn node_lines_carry_ids_and_relative_paths() {
        let db = open_db_in_memory().unwrap();
        // Seed with an absolute source path under a fake root; the DB path
        // determines the root.
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a', 'Alpha', 'code', 'C:/repo/src/alpha.rs');
            INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file) VALUES
                ('a', 'a', 'calls', 'EXTRACTED', 1.0, 'C:/repo/src/alpha.rs');",
        )
        .unwrap();
        let (text, _, _, _) = query_graph(
            &db,
            "C:/repo/.astria/db.sqlite",
            "Alpha",
            "bfs",
            2,
            2000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        assert!(
            text.contains("[id=a src=src/alpha.rs"),
            "expected id and root-relative path in output, got: {text}"
        );
        assert!(!text.contains("C:/repo/src"), "absolute path must not leak");
    }

    #[test]
    fn tiny_budget_rejects_incomplete_records() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('a', 'Alpha', 'code', 'f.rs', 'docstring-a'),
                ('b', 'Beta', 'code', 'f.rs', 'docstring-b'),
                ('c', 'Gamma', 'code', 'f.rs', 'docstring-c'),
                ('d', 'Delta', 'code', 'f.rs', 'docstring-d');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('a', 'b', 'calls', 'EXTRACTED', 'f.rs'),
                ('a', 'c', 'calls', 'EXTRACTED', 'f.rs'),
                ('a', 'd', 'calls', 'EXTRACTED', 'f.rs');",
        )
        .unwrap();
        let result = query_graph(
            &db,
            ":memory:edgebudget",
            "Alpha",
            "bfs",
            2,
            1,
            false,
            0.0,
            0,
            false,
        );
        assert!(result.is_err(), "one token cannot hold a complete response");
    }

    #[test]
    fn plural_terms_match_singular_labels() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('x', 'community_handler', 'code', 'src/x.rs');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('x', 'x', 'calls', 'EXTRACTED', 'src/x.rs');",
        )
        .unwrap();
        let (text, nodes, _, _) = query_graph(
            &db,
            ":memory:plural",
            "communities",
            "bfs",
            1,
            2000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        assert!(
            nodes > 0 && text.contains("community_handler"),
            "got: {text}"
        );
    }

    #[test]
    fn docstring_matches_contribute_to_seeds() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file, docstring) VALUES
                ('d', 'Helper', 'code', 'src/d.rs', 'Handles authentication tokens for the API');
            INSERT INTO edges (source, target, relation, confidence, source_file) VALUES
                ('d', 'd', 'calls', 'EXTRACTED', 'src/d.rs');",
        )
        .unwrap();
        let (_, nodes, _, _) = query_graph(
            &db,
            ":memory:docseed",
            "authentication",
            "bfs",
            1,
            2000,
            false,
            0.0,
            0,
            false,
        )
        .unwrap();
        assert!(nodes > 0, "docstring content should seed the node");
    }

    #[test]
    fn cursor_pages_through_truncated_nodes() {
        let db = open_db_in_memory().unwrap();
        let mut sql = String::from("INSERT INTO nodes (id, label, file_type, source_file) VALUES ");
        for i in 0..30 {
            if i > 0 {
                sql.push(',');
            }
            sql.push_str(&format!("('n{i:02}', 'Node{i:02}', 'code', 'f.rs')"));
        }
        sql.push(';');
        // Make all fixture nodes reachable, with explicit confidence scores,
        // so pagination measures a traversal larger than the five seeds.
        for i in 1..30 {
            sql.push_str(&format!("INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file) VALUES ('n00','n{i:02}','calls','EXTRACTED',1.0,'f.rs');"));
        }
        db.execute_batch(&sql).unwrap();
        let key = ":memory:cursor";

        let (text1, _, _, next1) =
            query_graph(&db, key, "Node", "bfs", 1, 120, false, 0.0, 0, false).unwrap();
        assert!(next1.is_some(), "records cannot fit in 120 tokens: {text1}");
        assert!(text1.contains("cursor"));

        let (text2, _, _, _) = query_graph(
            &db,
            key,
            "Node",
            "bfs",
            1,
            120,
            false,
            0.0,
            next1.unwrap(),
            false,
        )
        .unwrap();
        // The second page must start past the first page's records.
        assert_ne!(
            text1.lines().nth(2),
            text2.lines().nth(2),
            "cursor should advance the record window"
        );
    }

    #[test]
    fn detail_high_drops_inferred_edges() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('a', 'Alpha', 'code', 'f.rs'),
                ('b', 'Beta', 'code', 'f.rs'),
                ('c', 'Gamma', 'code', 'f.rs');
            INSERT INTO edges (source, target, relation, confidence, confidence_score, source_file) VALUES
                ('a', 'b', 'calls', 'EXTRACTED', 1.0, 'f.rs'),
                ('b', 'c', 'calls', 'INFERRED', 0.7, 'f.rs');",
        )
        .unwrap();
        let key = ":memory:detail";
        let (all, _, _, _) =
            query_graph(&db, key, "Beta", "bfs", 1, 4000, false, 0.0, 0, false).unwrap();
        assert!(all.contains("Gamma"), "default tier keeps inferred edges");

        let (high, _, _, _) =
            query_graph(&db, key, "Beta", "bfs", 1, 4000, false, 0.9, 0, false).unwrap();
        assert!(
            !high.contains("NODE Gamma"),
            "high tier must not traverse inferred edges, got: {high}"
        );
    }

    #[test]
    fn repo_map_is_ranked_deterministic_and_budgeted() {
        let db = open_db_in_memory().unwrap();
        let mut sql = String::from(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES
                ('h', 'Hub', 'code', 'src/hub.rs'),
                ('h1', 'Hub1', 'code', 'src/hub.rs'),
                ('h2', 'Hub2', 'code', 'src/hub.rs'),
                ('o', 'Orphan', 'code', 'src/orphan.rs'),
                ('p', 'Peer', 'code', 'src/peer.rs');",
        );
        // hub.rs heavily connected to peer.rs; orphan.rs isolated
        let sources = ["h", "h1", "h2", "h", "h1"];
        for src in sources {
            sql.push_str(&format!(
                "; INSERT INTO edges (source, target, relation, confidence, source_file) VALUES ('{src}', 'p', 'calls', 'EXTRACTED', 'src/hub.rs')"
            ));
        }
        db.execute_batch(&sql).unwrap();
        let key = ":memory:map";

        let (map1, shown1) = repo_map(&db, key, 4000, 0.0).unwrap();
        let (map2, _) = repo_map(&db, key, 4000, 0.0).unwrap();
        assert_eq!(map1, map2, "map must be deterministic");
        assert!(map1.contains("src/hub.rs"));
        assert!(map1.contains("Orphan"), "isolated files still appear");
        assert_eq!(shown1, 3);

        let (small, small_shown) = repo_map(&db, key, 1, 0.0).unwrap();
        assert!(small_shown < shown1, "tiny budget shows fewer files");
        assert!(small.contains("truncated"), "truncation is declared");
        assert!(small.contains("Hub"), "the top-ranked file always fits");
    }
}

#[cfg(test)]
mod scope_match {
    use super::*;
    #[test]
    fn qualified_names_match_id_scope_tokens_and_tails() {
        // qualified name -> scope slug + method tail
        assert!(qualified_scope_match(
            "BaseCommand.get_usage",
            "src_click_core_basecommand::get_usage"
        ));
        // same-name symbol from the wrong class must not match
        assert!(!qualified_scope_match(
            "BaseCommand.get_usage",
            "src_click_core_command::get_usage"
        ));
        // bare class name matches its scope token
        assert!(qualified_scope_match(
            "BaseCommand",
            "src_click_core_basecommand::invoke"
        ));
        // prose must never match
        assert!(!qualified_scope_match(
            "how the usage works",
            "src_click_core_basecommand::get_usage"
        ));
    }
}
