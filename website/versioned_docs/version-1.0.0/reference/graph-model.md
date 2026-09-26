---
sidebar_position: 6
title: Graph model
description: What the graph contains — node types, relation types, provenance (EXTRACTED / INFERRED / AMBIGUOUS), confidence scores, hyperedges, and learned edges.
keywords: [graph model, nodes, edges, relations, provenance, confidence, hyperedges, learned edges]
---

# Graph model

astria stores a property graph in SQLite (see [The .astria directory](./directory-layout)). This page enumerates everything that can appear in it: node types, relation types, and the provenance system that tells you which facts were found in source versus deduced.

## Nodes

Every node carries: a stable `id` (deterministic from the file path and symbol — the same code produces the same ids across runs), a `label`, a `node_type`, source location (`source_file` + `source_line`), an optional `docstring` and `signature` (source text up to the body — what the symbol is without opening the file), plus computed fields (`community` id, `degree_centrality`).

### Node types

| Type | Produced by | Meaning |
|---|---|---|
| `file` | AST extraction | One per analyzed source file |
| `function` / `class` | AST extraction | Code symbols (functions, methods, classes, structs, traits — the vocabulary varies per language; see [Language support](./language-support)) |
| `document` / `section` | Ingest + extraction | Markdown/text content: README, docs pages, wiki, transcripts, fetched URLs |
| `reference` | Extraction | Identifier-shaped string literals (env var names, snake_case keys, dotted/kebab/slash chains) so config/status-value usage is queryable |
| `rationale` | Memory ingest | Curated Q/A answers from `save-result`, linked to the code they cite |
| `package` | Manifest ingest | Cargo workspace members and internal path dependencies (`crate::*` nodes) |
| `mcp_server` / `mcp_command` / `mcp_package` / `env_var` | MCP config ingest | Servers declared in `.mcp.json`, `mcp_servers.json`, `claude_desktop_config.json` (env **names** only, never values) |
| `concept` / `entity` / `code` | LLM semantic enrichment | Concept nodes produced from docs/papers/images when an LLM backend is configured (see [Semantic enrichment](../guides/semantic-enrichment)) |

## Relations

| Relation | Meaning | Provenance |
|---|---|---|
| `calls` | A calls B | EXTRACTED (call sites) |
| `contains` | File/class contains symbol | EXTRACTED |
| `imports` | Import/require/use between files | EXTRACTED |
| `uses` | Identifier usage within a body | EXTRACTED |
| `references` | Node mentions a `reference` literal | EXTRACTED |
| `depends_on` | Document-level dependency (e.g. memory docs citing files) | EXTRACTED |
| `crate_depends_on` | Cargo workspace/path dependency (honors `package =` renames, `workspace = true`) | EXTRACTED |
| `requires_env` | MCP server command requires an env var (name only) | EXTRACTED |
| `similar_to` | Semantic similarity between embeddings (cosine-scored) | INFERRED |
| `learned` | Recurring (seed, discovered) query pair promoted by usage | INFERRED |
| `rationale_for` | Curated memory answer grounded in code nodes | EXTRACTED |
| `same_type_as` | Cross-repo: types sharing `(namespace, label)` in the global graph | INFERRED |
| `scip_impl` / `scip_typed` / `scip_def` / `scip_ref` | From an ingested SCIP index (`add --scip`) | EXTRACTED |
| `participate_in` / `shares_reference` | Hyperedge membership (see below) | EXTRACTED |
| *(backend-defined, e.g. `forks`)* | LLM enrichment of docs/papers/images | INFERRED |

## Provenance and confidence

Every edge is labeled with one of three provenance values, plus a numeric `confidence_score`:

- **EXTRACTED** — found directly in the source (AST match, manifest parse, SCIP index). Declared fact.
- **INFERRED** — deduced: embeddings, learned edges, global-graph type matching.
- **AMBIGUOUS** — plausible but unconfirmed (e.g. a name match that could collide).

You can always tell what was found versus deduced. High-fidelity traversals (`query --detail high`, `path --detail high`, `map --detail high`, MCP `repo_map`/`query_graph` fidelity tiers) keep only declared facts. Every `EDGE` line in query output is anchored with `@file:line` and every `NODE` with `src=file:line`.

## Hyperedges

N-ary node groups, produced deterministically at build time (no LLM):

- **`participate_in`** — one per community; its top-degree members.
- **`shares_reference`** — one per identifier-shaped literal referenced from ≥ 3 distinct files.

Hyperedges are consumed by `graph.json`, the report, the wiki, HTML hulls, and `explain`. See [Wiki and exports](../guides/wiki-and-exports#hyperedges-in-exports).

## Learned edges

The graph compounds in value as you query it: when the same (seed, discovered) node pair recurs across ≥ 2 distinct questions with 3+ total hits, the next `run`/`update` promotes it to a `learned` edge (INFERRED, hits-scored, provenance `query_history`). See [Learning from usage](./cli#learning-from-usage).

## Deduplication

Near-duplicate nodes (same symbol extracted under slightly different names) are merged at build time with MinHash/LSH blocking + Jaro-Winkler verification. `run --no-dedup` skips this.
