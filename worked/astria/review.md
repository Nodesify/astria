# Worked example: astria on itself

> **Historical note.** This review was produced with the pre-1.0 tool — then
> named `nodesify-graphify` (npm `@nodesify/graphify`), with the graph in
> `.graphify/`, 14 crates, and the ignore file `.graphifyignore`. Since the
> 1.0 rebrand the binary is `astria`, the package `@nodesify/astria`, the
> graph lives in `.astria/`, the workspace has 15 crates, and the ignore
> file is `.astriaignore`. Names below are as they were when measured.

**Corpus:** this repository — 14 Rust crates at the time, a Node.js CLI, skill files, markdown docs.
156 files tracked, **1,708 nodes / 7,965 edges / 212 communities**, modularity 0.536.
Built with `run --embed --wiki` (similarity edges on), benchmark: **78.8x** fewer tokens per query vs reading the corpus.

**Files:** `graph_report.md` (as generated), `wiki_index.md` (wiki entry point — 222 articles total, not copied in full), `quality-results.json` (golden-QA scores, below). The full `graph.json` (2.7 MB) is no longer stored in the repo — regenerate it with the command under **Reproduce**.

**Reproduce:** `astria run . --embed --wiki` in the repo root (as run pre-1.0: `nodesify-graphify run . --embed --wiki`).

## What the graph got right

- **Key Files is accurate.** The top hub files — `graphify-analyze/affected.rs`, `graphify-analyze/lib.rs`, `graphify-semantic/lib.rs`, `graphify-query/lib.rs`, `graphify-napi/lib.rs` — are exactly where the blast-radius, analysis, LLM, and query machinery lives. Anyone orienting here would start in the right place.
- **Real semantic clusters exist.** `export_wiki.rs` (51 nodes, cohesion 0.48) formed around the wiki-export feature; the OpenCode skill community (36 nodes, cohesion **0.85**) is genuinely tight. The `similar_to` edges did this — before embeddings, communities fragmented along file lines (~400 communities).
- **Cross-language semantic bridging.** `std::chrono::milliseconds ↔ std::time::systemtime::now` (similar_to, C++ fixture ↔ Rust code) is a true conceptual match the AST alone would never make. `skill.md ↔ index.md` linking the docs semantically is also real.
- **God nodes are the real load-bearing symbols**: `call_tool()` (MCP dispatch, degree 88), `build_payload()` (LLM request assembly), `installPlatform()` (the CLI's platform installer). Excluding call stubs works — `get`/`join` don't dominate.
- **The wiki is navigable.** Every index link resolves (e2e-tested); community articles carry the EXTRACTED/INFERRED audit trail.

## What the graph got wrong

- **Community labels are representative symbol names, not themes.** "sample.cpp", "path", "log", "get()" are label-propagation artifacts — real clusters, unhelpful names. A thematic labeler (top distinctive terms) would help.
- **Test fixtures pollute the orientation view.** The C++/Go/etc. `sample.*` fixture files are ~15% of the graph and win several community slots. For understanding this repo they're noise; a `.graphifyignore` for `tests/fixtures/` would sharpen the picture (kept here to show the honest default; the sample fixtures have since been removed from the repo tree).
- **`lib.rs` is the top god node (degree 305) and that's crude.** It's a filename-node aggregating every module's lib.rs — technically real, semantically thin.
- **Some surprising connections are stub noise** (`perm_params() → std::sync::lazylock::new` via unresolved std calls). Novelty ranking surfaces them because both endpoints sit in large communities.
- **212 communities for 1.7k nodes is fragmented.** Deterministic label propagation over-fragments relative to Leiden; the consolidation from similarity edges (401 → 212) helped but didn't finish the job.

## Retrieval quality (measured)

After this review, a 35-question golden QA set with ground-truth files
(`scripts/bench/quality/`) was run through the real query engine against
this same graph — full results in
[`quality-results.json`](./quality-results.json) beside this file (0.8.0-era
engine; the docs site's benchmark page tracks the current snapshot):
recall@5 **8.6%**, recall@10 **17.1%**, MRR **0.094**.
*(After the ranking fixes that this review's miss mode pointed at —
relevance-ranked answers, stopword filtering, code-over-prose prior — the
same set measures recall@5 **65.7%**, recall@10 **85.7%**, MRR **0.537**.)*

The miss mode is the same story this review tells qualitatively, now with
numbers: answers are ordered by hub degree rather than question relevance,
and doc-heading nodes crowd out implementing code inside the token budget —
"how does the token reduction benchmark work" returns `benchmarks.md`
headings and never reaches `crates/astria-napi/src/benchmark.rs`. Cheap
answers, weak file-level precision. The two engine fixes this indicts:
relevance-rank answer nodes, cap doc-heading seeds.

## Verdict

For a 156-file polyglot repo: the hub/file/feature-cluster signal is strong and immediately useful; community naming and fixture noise are the visible weaknesses. The token benchmark (78.8x) is measured, not estimated — corpus bytes vs actual query output.
