# Astria reliability implementation plan

**Goal:** Implement all findings accepted in the project review.

**Architecture:** Keep the Rust workspace and SQLite storage. Consolidate lifecycle ownership, retain original graph evidence, share query orchestration across CLI/MCP, and derive discovery from the extraction language registry. Keep semantic enrichment explicit and reproducible.

**Constraints:** Shared workspace; no worktrees, commits, or test creation/execution. Verify through compilation, builds, and source review. Skip conflicting edits and report them.

- [x] Graph updates: publish file manifests with successful graph writes; reconcile references against the complete symbol corpus and retain unchanged callers.
- [x] Query module: preserve actual edge identity/orientation, distinguish evidence from popularity, refresh cached snapshots across processes, share hybrid retrieval across CLI/MCP.
- [x] Languages: use a single registry for detection, extraction, and generated capability documentation.
- [x] Semantics: explicit activation, effective-configuration cache fingerprints, identical merging of cached/fresh results.
- [x] Benchmarks: count failures, distinguish hit rate and recall, gate source changes, provide external-corpus and targeted-search comparisons without inventing measurements.
- [x] CLI: make global registration independent of wiki export.
- [x] Documentation: update current behavior and position Astria around fresh, source-grounded code context.
- [x] Verification: Rust compilation, TypeScript build, benchmark syntax checks, documentation build, final source/diff review. No tests.

## Verification and operational notes

- Rust workspace native build passed (`cargo build --workspace --locked`).
- Embedded and non-embedded compilation passed; strict workspace Clippy passed.
- CLI TypeScript build and documentation production build passed.
- Docs-sync, benchmark JavaScript syntax checks, and whitespace checks passed.
- Refreshed this repository graph with the built native module: 2,503 nodes, 16,427 edges, 132 communities, zero LLM calls. Existing cached local embeddings were refreshed.
- No test suites or benchmark suites were run. No new benchmark results are claimed.
- Queries load a fresh O(V + E) snapshot rather than reuse a process-global cache.
- Explicit backend selection is now required for network enrichment. Core facts/manifest publication is atomic; derived passes and file exports are subsequent stages.
