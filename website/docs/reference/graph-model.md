---
sidebar_position: 6
title: Graph model
description: What the graph contains — node types, relation types, provenance (EXTRACTED / RESOLVED / INFERRED / SEMANTIC / AMBIGUOUS), confidence scores, hyperedges, and learned edges.
keywords: [graph model, nodes, edges, relations, provenance, confidence, hyperedges, learned edges]
---

# Graph model

astria stores a property graph in SQLite (see [The .astria directory](./directory-layout)). This page enumerates everything that can appear in it: node types, relation types, and the provenance system that tells you which facts were found in source versus deduced.

## Nodes

Every node carries: a stable `id` (deterministic from the file path and symbol — the same code produces the same ids across runs), a `label`, a `node_type`, source location (`source_file` + `source_line`), an optional `docstring` (doc comments where the language has them — `///` items and `//!` modules in Rust, body docstrings in Python/JS) and `signature` (source text up to the body — what the symbol is without opening the file), plus computed fields (`community` id, `degree_centrality`).

### Node types

| Type | Produced by | Meaning |
|---|---|---|
| `file` | AST extraction | One per analyzed source file |
| `function` / `class` | AST extraction | Code symbols (functions, methods, classes, structs, traits — the vocabulary varies per language; see [Language support](./language-support)). Rust `pub`/documented `const`/`static` items are extracted with the initializer verbatim as the signature (extraction type `constant`, stored as code) so value questions ("which model", "what threshold") are answerable |
| `document` / `section` | Ingest + extraction | Markdown/text content: README, docs pages, wiki, transcripts, fetched URLs |
| `reference` | Extraction | Identifier-shaped string literals (env var names, snake_case keys, dotted/kebab/slash chains) so config/status-value usage is queryable |
| `rationale` | Extraction | An explanatory code comment captured as a node, linked by `rationale_for` to the code it explains |
| `package` | Manifest ingest | Cargo workspace members and internal path dependencies (`crate::*` nodes) |
| `mcp_server` / `mcp_command` / `mcp_package` / `env_var` | MCP config ingest | Servers declared in `.mcp.json`, `mcp_servers.json`, `claude_desktop_config.json` (env **names** only, never values) |
| `concept` / `entity` / `code` | LLM semantic enrichment | Concept nodes produced from docs/papers/images when an LLM backend is configured (see [Semantic enrichment](../guides/semantic-enrichment)) |

## Relations

| Relation | Meaning | Provenance |
|---|---|---|
| `calls` | A calls B | RESOLVED when the callee name binds to exactly one definition; INFERRED when it cannot resolve (the call site is extracted either way) |
| `contains` | File/class contains symbol | EXTRACTED |
| `imports` | Import/require/use between files | EXTRACTED |
| `uses` | Identifier usage within a body | EXTRACTED |
| `references` | Node mentions a `reference` literal, or a memory document cites a node (from `save-result`, which inserts the doc as a document node immediately) | EXTRACTED |
| `depends_on` | Document-level dependency (e.g. memory docs citing files) | EXTRACTED |
| `crate_depends_on` | Cargo workspace/path dependency (honors `package =` renames, `workspace = true`) | EXTRACTED |
| `entry_point` | Cross-layer: a package → its conventional entry file (`src/lib.rs`, `index.ts`, `__init__.py`, …) | EXTRACTED |
| `ffi_binding` | Cross-layer: a TS/JS symbol importing the napi binding → the Rust function behind it | EXTRACTED |
| `requires_env` | MCP server command requires an env var (name only) | EXTRACTED |
| `similar_to` | Semantic similarity between embeddings (cosine-scored) | INFERRED |
| `learned` | Recurring (seed, discovered) query pair promoted by usage | INFERRED |
| `rationale_for` | A code-comment rationale linked to the code it explains | EXTRACTED |
| `same_type_as` | Cross-repo: same-label type declarations in the global graph (name-based unification) | INFERRED |
| `scip_impl` / `scip_typed` / `scip_def` / `scip_ref` | From an ingested SCIP index (`add --scip`) | EXTRACTED |
| `participate_in` / `shares_reference` | Hyperedge membership (see below) | EXTRACTED |
| `implements` / `relates_to` | LLM semantic enrichment of docs/papers/images; relations are validated against a fixed allowlist (`implements`, `depends_on`, `relates_to`, `uses`, `contains`) and anything outside it clamps to `relates_to` | SEMANTIC |

## Provenance and confidence

Every edge is labeled with a provenance value, plus a numeric `confidence_score`:

- **EXTRACTED** — found directly in the source (AST match, manifest parse, SCIP index). Declared fact.
- **RESOLVED** — a call expression extracted from source whose bare name binds to exactly one definition during reference resolution. Trustworthy for impact analysis, but the binding is name inference rather than compiler resolution, so it deliberately sits below EXTRACTED: high-fidelity tiers and EXTRACTED-only checks (like health's file-cycle detection) still exclude it.
- **INFERRED** — deduced: unresolvable call stubs, embeddings, learned edges, hyperedges, global-graph type matching.
- **SEMANTIC** — produced by LLM enrichment: concept nodes and their edges extracted by the semantic backend, with relations validated against a fixed allowlist. Retrieval ranks it between INFERRED and AMBIGUOUS when no numeric score is present.
- **AMBIGUOUS** — plausible but unconfirmed (e.g. a name match that could collide).
- **DECLARED** — recognized alongside EXTRACTED for externally declared facts (not produced by the standard pipeline; it appears in externally assembled or merged graphs). Where present it ranks just above EXTRACTED.

You can always tell what was found versus deduced. High-fidelity traversals (`query --detail high`, `path --detail high`, `map --detail high`, MCP `repo_map`/`query_graph` fidelity tiers) keep only declared facts. Every `EDGE` line in query output is anchored with `@file:line` and every `NODE` with `src=file:line`.

Semantic edges carry the `SEMANTIC` provenance with a null score by default; under the [Jev judge layer](../guides/semantic-enrichment#jev-judge-layer) they gain the judge's calibrated existence probability (0–1) in `confidence_score`, and edges the judge rejects are dropped instead of published.

## Hyperedges

N-ary node groups, produced deterministically at build time (no LLM):

- **`participate_in`** — one per community; its top-degree members.
- **`shares_reference`** — one per identifier-shaped literal referenced from ≥ 3 distinct files.

Hyperedges are consumed by `graph.json`, the report, the wiki, HTML hulls, and `explain`. See [Wiki and exports](../guides/wiki-and-exports#hyperedges-in-exports).

## Learned edges

The graph compounds in value as you query it: when the same (seed, discovered) node pair recurs across ≥ 2 distinct questions with 3+ total hits, the next `run`/`update` promotes it to a `learned` edge (INFERRED, hits-scored, provenance `query_history`). See [Learning from usage](./cli#learning-from-usage).

## Deduplication

Near-duplicate nodes (same symbol extracted under slightly different names) are merged at build time with MinHash/LSH blocking + Jaro-Winkler verification. `run --no-dedup` skips this.
