---
sidebar_position: 6
title: Architecture
---

# Architecture

nodesify-graphify turns source code into a queryable knowledge graph. It uses AST-based extraction via tree-sitter for deterministic, fast analysis, stored in a SQLite database.

The project is a Rust workspace with 14 domain-specific crates and a Node.js CLI.

- **Language**: Rust 2021
- **Build system**: Cargo + npm
- **Core dependencies**: `rusqlite` (persistence), `tree-sitter` (AST parsing), `petgraph` (graph algorithms), `napi-rs` (Node.js bindings)

## Pipeline

```
detect() → extract() → enrich_with_semantics() → build() → dedup_nodes() → cluster() → analyze() → report()
```

The pipeline is orchestrated in `crates/graphify-napi/src/pipeline.rs`.

1. **detect()** (`graphify-detect`): Discovers files, classifies them (Code, Document, etc.), and uses a SHA-256 manifest to identify changed files since the last run.
2. **extract()** (`graphify-extract`): Performs AST-based extraction using tree-sitter. Supports 21 languages with per-language configurations in `src/langs/`.
3. **enrich_with_semantics()** (`graphify-semantic`, optional): When an LLM backend is configured, extracts topics, concepts, and entities (including from images via vision) concurrently and caches the results.
4. **build()** (`graphify-build`): Merges extracted nodes and edges into the SQLite graph database, handles deduplication and cross-file reference resolution.
5. **cluster()** (`graphify-cluster`): Performs community detection using the deterministic label propagation algorithm (via `petgraph`) and updates the `community` attribute on nodes.
6. **analyze()** (`graphify-analyze`): Analyzes the graph to find "god nodes" (call stubs excluded), surprising cross-community connections, blast radius, and generates suggested questions.
7. **report()** (`graphify-report`): Generates a plain-language `graph_report.md` summarizing the graph's structure and insights.

Each stage is a pure function in its own crate; semantic enrichment is optional and activates when an LLM backend is configured.

## Crate responsibilities

| Crate | Responsibility |
| :--- | :--- |
| `graphify-core` | Shared types (`FileType`, `GraphStats`), `GraphifyError`, SQLite schema + migrations, path validation, sanitization, sensitive-path denylist. |
| `graphify-paths` | Path normalization and `.graphify` directory management. |
| `graphify-detect` | File system scanning, `.graphifyignore` support, and incremental change detection via SHA-256 hashes. |
| `graphify-extract` | Tree-sitter AST traversal logic. Each language defines its own extraction rules (nodes, edges, docstrings). |
| `graphify-build` | Persistent graph assembly; entity dedup (MinHash/LSH blocking + Jaro-Winkler verify) in `dedup.rs`. |
| `graphify-cluster` | Deterministic community detection (stable labels, cohesion, modularity) using `petgraph`. |
| `graphify-analyze` | God nodes, ranked surprising cross-community connections, blast radius (`affected.rs`, reverse reachability). |
| `graphify-query` | Query engine: BFS/DFS (optionally directed), shortest path, explain, token-based node scoring, per-path graph cache. |
| `graphify-mcp` | MCP stdio server exposing the graph to AI agents. |
| `graphify-report` | Markdown generation for the final user-facing report. |
| `graphify-semantic` | LLM semantic extraction, multi-backend (Claude / OpenAI-compatible / Gemini) with vision, chunking, and output validation. |
| `graphify-ingest` | URL ingestion (arXiv/tweet/webpage/image) with SSRF protection. |
| `graphify-pdf` | PDF text extraction. |
| `graphify-napi` | The bridge between Rust and Node.js: pipeline orchestration, query surface, merge/diff, JSON/HTML/GraphML/tree export. |
| `graphify-cli` | The Node.js-based user interface, responsible for argument parsing and installing AI skills. |

## Data model

### SQLite schema

The graph is stored in `.graphify/db.sqlite` with the following tables:

- `nodes`: `id`, `label`, `file_type`, `source_file`, `source_line`, `docstring`, `community`
- `edges`: `source`, `target`, `relation`, `confidence`, `confidence_score`, `source_file`, `source_line`
- `communities`: detected community labels and cohesion scores
- `file_manifest`: `path`, `hash`, `last_extracted_at` — used for incremental updates
- `extraction_cache`: cached per-file extraction results keyed by content hash
- `pipeline_runs`: one row per pipeline run (stage timing, version stamp)
- `query_history`: `question`, `answer`, `queried_at`
- `_meta`: schema version and other bookkeeping

### Relationship types

- `Calls` — function or method invocation
- `Imports` — module or file level dependency
- `Uses` — variable or type usage
- `Defines` — containment (e.g., class defines a method)
- `Inherits` — class inheritance or interface implementation

## Persistence and performance

- **SQLite** — chosen for its zero-config nature and robust ACID properties, making it perfect for local analysis.
- **Incremental rebuilds** — the system only re-extracts files that have changed, drastically reducing analysis time for large projects.
- **napi-rs** — provides near-native performance for the CLI while maintaining the ease of use of an npm package.

Design docs: [design spec](https://github.com/Nodesify/nodesify-graphify/blob/main/docs/superpowers/specs/2026-04-30-nodesify-graphify-rewrite-design.md) and [implementation plan](https://github.com/Nodesify/nodesify-graphify/blob/main/docs/superpowers/plans/2026-04-30-nodesify-graphify-implementation.md).
