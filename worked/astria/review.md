# Worked example: nodesify-graphify on itself

**Corpus:** this repository — 14 Rust crates, a Node.js CLI, skill files, markdown docs.
156 files tracked, **1,708 nodes / 7,965 edges / 212 communities**, modularity 0.536.
Built with `run --embed --wiki` (similarity edges on), benchmark: **78.8x** fewer tokens per query vs reading the corpus.

**Files:** `graph_report.md` (as generated), `graph.json` (full graph), `wiki_index.md` (wiki entry point — 222 articles total, not copied in full).

**Reproduce:** `nodesify-graphify run . --embed --wiki` in the repo root.

## What the graph got right

- **Key Files is accurate.** The top hub files — `graphify-analyze/affected.rs`, `graphify-analyze/lib.rs`, `graphify-semantic/lib.rs`, `graphify-query/lib.rs`, `graphify-napi/lib.rs` — are exactly where the blast-radius, analysis, LLM, and query machinery lives. Anyone orienting here would start in the right place.
- **Real semantic clusters exist.** `export_wiki.rs` (51 nodes, cohesion 0.48) formed around the wiki-export feature; the OpenCode skill community (36 nodes, cohesion **0.85**) is genuinely tight. The `similar_to` edges did this — before embeddings, communities fragmented along file lines (~400 communities).
- **Cross-language semantic bridging.** `std::chrono::milliseconds ↔ std::time::systemtime::now` (similar_to, C++ fixture ↔ Rust code) is a true conceptual match the AST alone would never make. `skill.md ↔ index.md` linking the docs semantically is also real.
- **God nodes are the real load-bearing symbols**: `call_tool()` (MCP dispatch, degree 88), `build_payload()` (LLM request assembly), `installPlatform()` (the CLI's platform installer). Excluding call stubs works — `get`/`join` don't dominate.
- **The wiki is navigable.** Every index link resolves (e2e-tested); community articles carry the EXTRACTED/INFERRED audit trail.

## What the graph got wrong

- **Community labels are representative symbol names, not themes.** "sample.cpp", "path", "log", "get()" are label-propagation artifacts — real clusters, unhelpful names. A thematic labeler (top distinctive terms) would help.
- **Test fixtures pollute the orientation view.** The C++/Go/etc. `sample.*` fixture files are ~15% of the graph and win several community slots. For understanding this repo they're noise; a `.graphifyignore` for `tests/fixtures/` would sharpen the picture (kept here to show the honest default).
- **`lib.rs` is the top god node (degree 305) and that's crude.** It's a filename-node aggregating every module's lib.rs — technically real, semantically thin.
- **Some surprising connections are stub noise** (`perm_params() → std::sync::lazylock::new` via unresolved std calls). Novelty ranking surfaces them because both endpoints sit in large communities.
- **212 communities for 1.7k nodes is fragmented.** Deterministic label propagation over-fragments relative to Leiden; the consolidation from similarity edges (401 → 212) helped but didn't finish the job.

## Verdict

For a 156-file polyglot repo: the hub/file/feature-cluster signal is strong and immediately useful; community naming and fixture noise are the visible weaknesses. The token benchmark (78.8x) is measured, not estimated — corpus bytes vs actual query output.
