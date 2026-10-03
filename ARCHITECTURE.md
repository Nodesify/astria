# Astria Architecture Reference

astria turns source code into a queryable knowledge graph. It uses AST-based extraction via tree-sitter for deterministic, fast analysis, stored in a SQLite database.

## Overview

The project is structured as a Rust workspace with 20 domain-specific crates and a Node.js CLI.

**Language**: Rust 2021
**Build system**: Cargo + npm
**Core dependencies**: `rusqlite` (persistence), `tree-sitter` (AST parsing), `petgraph` (graph algorithms), `napi-rs` (Node.js bindings)

## Pipeline

```
detect() → extract() → enrich_with_semantics() → build() → dedup_nodes() → embed() (optional --embed) → cluster() → analyze() → report()
```

The pipeline is orchestrated in `crates/astria-napi/src/pipeline.rs`.

1.  **detect()** (`astria-detect`): Discovers files, classifies them (Code, Document, etc.), and uses a SHA-256 manifest to identify changed files since the last run.
2.  **extract()** (`astria-extract`): Performs AST-based extraction using tree-sitter. Uses 42 registered language configurations; discovery and parser selection share `astria-core/src/languages.rs`, with AST rules in `src/langs/`.
3.  **enrich_with_semantics()** (`astria-semantic`, optional): When `--backend` or `ASTRIA_LLM_BACKEND` explicitly selects a backend, extracts topics, concepts, and entities (including from images via vision) concurrently and caches the results. With `--judge jev`, a TypeSafe System One judge layer wraps the engine: batch file gating before extraction, per-file re-judging of relations/node types with calibrated `confidence_score` on edges, and suggested-question ranking.
4.  **build()** (`astria-build`): Publishes extracted nodes and edges into SQLite. The extraction reference pass reconciles cross-file references before publication; semantic entity deduplication runs as a derived pass. Code and test definitions, packages, rationale nodes and file identities are excluded from fuzzy merging: identical method names in different scopes remain separate definitions.
5.  **embed()** (`astria-embed`, optional `--embed`): Computes local node embeddings (fastembed/ONNX, no API key) and adds `similar_to` edges ahead of the cluster() stage, so community detection consumes semantic similarity.
6.  **cluster()** (`astria-cluster`): Performs community detection using the deterministic label propagation algorithm (via `petgraph`) and updates the `community` attribute on nodes.
7.  **analyze()** (`astria-analyze`): Analyzes the graph to find "god nodes" (call stubs excluded), surprising cross-community connections, blast radius, and generates suggested questions.
8.  **report()** (`astria-report`): Generates a plain-language `graph_report.md` summarizing the graph's structure and insights.

## Update and query consistency

AST parsing is incremental: unchanged source reuses its versioned extraction cache. The extraction reference pass reconciles the complete current corpus, so adding, removing, or renaming a definition also updates callers from unchanged files. Name-based resolution is still `INFERRED`; deterministic execution does not make a guessed target a declared fact.

Validated file-owned graph facts, the file manifest, `_meta.graph_published_at`, the covered git HEAD (`_meta.git_head`), and a fresh publication generation (`_meta.graph_generation`) commit in one SQLite transaction — so a core publication can never leave snapshot caches keyed on the previous state, even if a later stage fails. Derived passes run after the core commit and advance the generation immediately after any pass that actually changes content (dedup merges, deep links, embedding edges, learned edges, community memberships, community labels); unchanged runs reuse the previous generation instead of minting a new one. Clustering-only reruns publish through the same workflow (stamped report, `graph.json`, `generation.txt`) as full pipelines. Extraction or semantic extraction errors leave that core graph and manifest unadvanced; derived passes rerun on subsequent updates, so a failed derived pass can be retried.

Merge-gate freshness is commit identity, not timestamps: the gate compares `_meta.git_head` with the repository's current HEAD and re-hashes every manifest file against the graph's own versioned content scheme, so a graph built from a dirty tree, an older commit, or one whose sources drifted since publication fails `graph-covers-head` (query headers carry the cheaper stat-only version of the same signal: modified, deleted, and size-changed files since publication).

Semantic caches include source inputs and non-secret effective backend, endpoint, model, judge, and prompt configuration. Cached and fresh semantic results use the same merge path. Community labels and deep links also fingerprint their effective inputs and configuration. Source changes invalidate deep edges; restoring them requires another run with `--deep`, which replays matching cache entries or generates fresh links.

CLI and MCP queries use the same hybrid retrieval path, served from a bounded process-wide snapshot cache: an LRU (default 3 entries, `ASTRIA_SNAPSHOT_CACHE_ENTRIES` 1..=16) keyed by database path AND publication generation (`_meta.graph_generation`). A hit shares an immutable in-memory snapshot — no O(V + E) reload; any generation advance (every committed graph mutation performs one) invalidates the key, and un-stamped databases bypass the cache entirely. The bound trades memory (each entry is a full node+edge snapshot) for multi-project MCP alternation latency. `--detail high` filters on evidence kind (`EXTRACTED`), independent of usage-adjusted scores; learned, name-resolved, and semantic edges remain inferred.

## Crate Responsibilities

| Crate | Responsibility |
| :--- | :--- |
| `astria-bolt` | Minimal hand-rolled Bolt client (PackStream, chunked framing, HELLO/RUN/PULL) for live Neo4j pushes — no driver dependency. |
| `astria-core` | Shared types (`FileType`, `GraphStats`), `AstriaError`, SQLite schema + migrations, path validation, sanitization, sensitive-path denylist. |
| `astria-paths` | Path normalization and `.astria` directory management. |
| `astria-detect` | File system scanning, `.astriaignore` support, and incremental change detection via SHA-256 hashes. |
| `astria-extract` | Tree-sitter AST traversal logic. Each language defines its own extraction rules (nodes, edges, docstrings). |
| `astria-embed` | Local semantic embeddings (fastembed/ONNX, no API key): `similar_to` edges and embedding-backed query recall. Optional (`--embed`). |
| `astria-build` | Persistent graph assembly; entity dedup (MinHash/LSH blocking + Jaro-Winkler verify) in `dedup.rs`. |
| `astria-cluster` | Deterministic community detection (stable labels, cohesion, modularity) using `petgraph`. |
| `astria-analyze` | God nodes, ranked surprising cross-community connections, blast radius (`affected.rs`, reverse reachability), code-health report (`health.rs`). |
| `astria-query` | Query engine: BFS/DFS (optionally directed), shortest path, explain, token-based node scoring, fresh SQLite snapshot per request (no process-global graph cache). |
| `astria-mcp` | MCP stdio server exposing the graph to AI agents. |
| `astria-report` | Markdown generation for the final user-facing report. |
| `astria-semantic` | LLM semantic extraction, multi-backend (Claude / OpenAI-compatible / Gemini) with vision, chunking, and output validation. `--judge jev` wraps the selected engine with a TypeSafe System One judge layer: batch file gating before extraction, per-file re-judging of relations/node types with calibrated `confidence_score` on edges, and suggested-question ranking. |
| `astria-ingest` | URL ingestion (arXiv/tweet/webpage/image) with SSRF protection. |
| `astria-audio` | Audio/video transcription via the external `whisper-cli` binary (whisper.cpp) and `ffmpeg` demux — transcript markdown feeds the document extractor. |
| `astria-office` | Office document text extraction: `.docx` (via `word/document.xml`) and `.xlsx` (via calamine) become markdown for the document extractor. |
| `astria-gws` | Google Workspace shortcut ingestion: `.gdoc`/`.gsheet`/`.gslides` links are exported through the Drive API and become document nodes. Missing credentials degrade to a notice. |
| `astria-export` | Graph export: JSON, interactive HTML, GraphML, SVG, Neo4j Cypher (with push), FalkorDB openCypher (with Redis push). |

| `astria-pdf` | PDF text extraction. |
| `astria-napi` | The bridge between Rust and Node.js: pipeline orchestration (semantic enrichment, community labeling, deep linking), query surface, merge/diff, JSON/HTML/GraphML/SVG/tree/Cypher export, live Neo4j push, health and risk reports. |
| `astria-cli` | The Node.js-based user interface, responsible for argument parsing and installing AI skills. |

## Data Models

### SQLite Schema

The graph is stored in `.astria/db.sqlite` with the tables `nodes`, `edges`, `hyperedges`, `communities`, `file_manifest`, `extraction_cache`, `pipeline_runs`, `query_history`, `node_embeddings` (populated by `--embed` builds), `query_pairs`, and `_meta`.

The column-level schema is generated from `crates/astria-core/src/db.rs` into [website/docs/explanation/architecture.md](website/docs/explanation/architecture.md) by `scripts/generate-schema-docs.mjs`; the docs-sync CI check rejects drift in either place.

### Relationship Types

Relations are stored lowercase — filter `--relation` with exactly these spellings.

Structural (AST extraction):

*   `calls`: Function or method invocation (including Ruby method and singleton-method invocations). Resolved targets are `EXTRACTED`; unresolved name-level targets `INFERRED`.
*   `contains`: File/class/symbol containment (there is no `Defines` relation).
*   `imports`: Module or file level dependency.
*   `uses`: Variable or type usage.
*   `references`: Document/markdown links, identifier mentions, and memory documents citing nodes (from `save-result`).
*   `rationale_for`: A comment rationale linked to the code it explains.
*   `crate_depends_on`: Cargo workspace/path-dependency topology (from `Cargo.toml` ingestion).
*   `requires_env`: An MCP server config and the environment-variable names it declares (names only, never values).
*   `scip_impl` / `scip_typed` / `scip_def` / `scip_ref`: Symbols and relations recorded from a simplified SCIP index (`add --scip`); unresolved targets become stubs.

Cross-layer (deterministic post-build passes; edges carry context `crosslayer` and are re-derived on every pipeline run):

*   `references`: A document naming a package as a whole token (crate tables, package lists) → that package's node.
*   `entry_point`: A package → its conventional entry file (`src/lib.rs`, `index.ts`, `__init__.py`, ...), so the package layer reaches code.
*   `ffi_binding`: A TS/JS symbol importing the napi binding → the Rust function behind it (napi-rs camelCase ↔ snake_case).

Semantic & learned (opt-in):

*   `similar_to`: Local embedding similarity (`--embed`); powers semantic query recall.
*   `implements`, `depends_on`, `relates_to`, `uses`, `contains`: LLM semantic extraction (validated against this allowlist; anything outside it clamps to `relates_to`). Under `--judge jev`, the TypeSafe judge layer re-chooses these relations from the allowlist and stores a calibrated existence probability in `edges.confidence_score`.
*   `learned`: Promoted from recurring query pairs (the memory feedback loop).

Hyperedges (n-ary, stored in the `hyperedges` table):

*   `participate_in`: Links a community's top-degree nodes to the community group.
*   `shares_reference`: Groups identifier-shaped string literals shared across files.

## Persistence & Performance

*   **SQLite**: Chosen for its zero-config nature and robust ACID properties, making it perfect for local analysis.
*   **Incremental Rebuilds**: The system only re-extracts files that have changed, drastically reducing analysis time for large projects.
*   **napi-rs**: Provides near-native performance for the CLI while maintaining the ease of use of an npm package.

## Language Support

Language names, extensions, and parser registration are defined once in `crates/astria-core/src/languages.rs`. Discovery and extraction consume that registry. The language-support documentation is generated by `scripts/generate-language-support.mjs` from the registry and actual AST configs; `scripts/check-docs-sync.mjs` rejects drift. Extraction rules are defined in `crates/astria-extract/src/langs/`. Each language module provides a `LanguageConfig` specifying which AST nodes represent classes, functions, and relationships.

The registry currently defines 42 language configurations; the authoritative per-language table (names, extensions, AST coverage) is generated from it into `website/docs/reference/language-support.md` — this count is drift-checked by `scripts/check-docs-sync.mjs`, and the hand-maintained list that used to live here was removed after it fell 17 languages behind the registry.
