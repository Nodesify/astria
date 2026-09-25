# Astria Architecture Reference

astria turns source code into a queryable knowledge graph. It uses AST-based extraction via tree-sitter for deterministic, fast analysis, stored in a SQLite database.

## Overview

The project is structured as a Rust workspace with 14 domain-specific crates and a Node.js CLI.

**Language**: Rust 2021
**Build system**: Cargo + npm
**Core dependencies**: `rusqlite` (persistence), `tree-sitter` (AST parsing), `petgraph` (graph algorithms), `napi-rs` (Node.js bindings)

## Pipeline

```
detect() → extract() → enrich_with_semantics() → build() → dedup_nodes() → cluster() → analyze() → report()
```

The pipeline is orchestrated in `crates/astria-napi/src/pipeline.rs`.

1.  **detect()** (`astria-detect`): Discovers files, classifies them (Code, Document, etc.), and uses a SHA-256 manifest to identify changed files since the last run.
2.  **extract()** (`astria-extract`): Performs AST-based extraction using tree-sitter. Supports 21 languages with per-language configurations in `src/langs/`.
3.  **enrich_with_semantics()** (`astria-semantic`, optional): When an LLM backend is configured, extracts topics, concepts, and entities (including from images via vision) concurrently and caches the results.
4.  **build()** (`astria-build`): Merges extracted nodes and edges into the SQLite graph database, handles deduplication and cross-file reference resolution.
5.  **cluster()** (`astria-cluster`): Performs community detection using the deterministic label propagation algorithm (via `petgraph`) and updates the `community` attribute on nodes.
6.  **analyze()** (`astria-analyze`): Analyzes the graph to find "god nodes" (call stubs excluded), surprising cross-community connections, blast radius, and generates suggested questions.
7.  **report()** (`astria-report`): Generates a plain-language `graph_report.md` summarizing the graph's structure and insights.

## Crate Responsibilities

| Crate | Responsibility |
| :--- | :--- |
| `astria-core` | Shared types (`FileType`, `GraphStats`), `AstriaError`, SQLite schema + migrations, path validation, sanitization, sensitive-path denylist. |
| `astria-paths` | Path normalization and `.astria` directory management. |
| `astria-detect` | File system scanning, `.astriaignore` support, and incremental change detection via SHA-256 hashes. |
| `astria-extract` | Tree-sitter AST traversal logic. Each language defines its own extraction rules (nodes, edges, docstrings). |
| `astria-build` | Persistent graph assembly; entity dedup (MinHash/LSH blocking + Jaro-Winkler verify) in `dedup.rs`. |
| `astria-cluster` | Deterministic community detection (stable labels, cohesion, modularity) using `petgraph`. |
| `astria-analyze` | God nodes, ranked surprising cross-community connections, blast radius (`affected.rs`, reverse reachability). |
| `astria-query` | Query engine: BFS/DFS (optionally directed), shortest path, explain, token-based node scoring, per-path graph cache. |
| `astria-mcp` | MCP stdio server exposing the graph to AI agents. |
| `astria-report` | Markdown generation for the final user-facing report. |
| `astria-semantic` | LLM semantic extraction, multi-backend (Claude / OpenAI-compatible / Gemini) with vision, chunking, and output validation. |
| `astria-ingest` | URL ingestion (arXiv/tweet/webpage/image) with SSRF protection. |
| `astria-pdf` | PDF text extraction. |
| `astria-napi` | The bridge between Rust and Node.js: pipeline orchestration, query surface, merge/diff, JSON/HTML/GraphML/tree export. |
| `astria-cli` | The Node.js-based user interface, responsible for argument parsing and installing AI skills. |

## Data Models

### SQLite Schema

The graph is stored in `.astria/db.sqlite` with the following tables:

*   `nodes`: `id`, `label`, `file_type`, `source_file`, `source_line`, `docstring`, `community`.
*   `edges`: `source`, `target`, `relation`, `confidence`, `confidence_score`, `source_file`, `source_line`.
*   `communities`: detected community labels and cohesion scores.
*   `file_manifest`: `path`, `hash`, `last_extracted_at`. Used for incremental updates.
*   `extraction_cache`: cached per-file extraction results keyed by content hash.
*   `pipeline_runs`: one row per pipeline run (stage timing, version stamp).
*   `query_history`: `question`, `answer`, `queried_at`.
*   `_meta`: schema version and other bookkeeping.

### Relationship Types

*   `Calls`: Function or method invocation.
*   `Imports`: Module or file level dependency.
*   `Uses`: Variable or type usage.
*   `Defines`: Containment (e.g., class defines a method).
*   `Inherits`: Class inheritance or interface implementation.

## Persistence & Performance

*   **SQLite**: Chosen for its zero-config nature and robust ACID properties, making it perfect for local analysis.
*   **Incremental Rebuilds**: The system only re-extracts files that have changed, drastically reducing analysis time for large projects.
*   **napi-rs**: Provides near-native performance for the CLI while maintaining the ease of use of an npm package.

## Language Support

Extraction rules are defined in `crates/astria-extract/src/langs/`. Each language module provides a `LanguageConfig` specifying which AST nodes represent classes, functions, and relationships.

Currently supported: Python, JS, TS, Rust, Go, Java, C, C++, Ruby, Swift, Kotlin, Scala, PHP, C#, Lua, Haskell, Elixir, Bash, Dart, Zig, CSS.
