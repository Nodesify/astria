---
sidebar_position: 2
title: MCP tools reference
description: The ten tools exposed by the astria MCP stdio server, with arguments, defaults, and example calls.
keywords: [mcp, tools, query_graph, repo_map, explain, affected, model context protocol]
---

# MCP tools reference

`astria mcp` runs an MCP stdio server (newline-delimited JSON-RPC 2.0, protocol `2025-06-18`, server name `astria`). Any MCP-capable agent — Claude Code, Codex, Cursor, … — can point at it and query the graph without shelling out to the CLI.

Point your agent's MCP config at it:

```json
{
  "mcpServers": {
    "astria": {
      "command": "astria",
      "args": ["mcp"]
    }
  }
}
```

The server's own instructions tell agents the intended flow: orient with `repo_map` first, ask natural-language questions with `query_graph`, drill into symbols with `explain`/`get_neighbors`, trace connections with `shortest_path`, and check `affected` **before** changing a node.

## Tools

### `query_graph`

BFS/DFS traversal of the knowledge graph for a natural-language question. Returns a compact subgraph context.

| Argument | Type | Default | Notes |
|---|---|---|---|
| `question` | string | — | **required** |
| `mode` | `bfs` \| `dfs` | `bfs` | |
| `depth` | integer | `2` | Maximum traversal depth |
| `budget` | integer | `2000` | Maximum query text tokens (`o200k_base`), including header and continuation metadata; excludes the MCP JSON envelope |
| `directed` | boolean | `false` | Follow edges only in stored direction (caller → callee, importer → module) |
| `detail` | `all` \| `high` | `all` | `high` keeps only `EXTRACTED`/`DECLARED` facts (a provenance-class filter, not a numeric threshold; inferred, semantic and learned edges are dropped) and prefers file-level nodes when ranking at equal relevance |
| `cursor` | integer | `0` | Continuation token from a previous truncated result |

CLI and MCP use the same query engine, including optional embedding recall when compiled with the `embed` feature and both node embeddings and a cached model are available. Queries never download a model. Each operation reloads a consistent SQLite graph snapshot, so long-running MCP sessions see subsequent database updates.

Undirected traversal can cross an edge backwards, but returned arrows always retain the stored source and target and the exact relationship traversed.

Truncated results report the next cursor value — re-run with `cursor` set to fetch the next slice of node and edge records. CLI and MCP use the same budgeted query text: `query_graph` returns it verbatim (`repo_map` and `shortest_path` append a short summary line after their text).

### `repo_map`

Aider-style repo map: files ranked by PageRank over the reference graph, with top symbols per file. One budgeted blob to orient on a codebase.

| Argument | Type | Default | Notes |
|---|---|---|---|
| `budget` | integer | `2000` | Approximate size cap — the map is cut at ~3 characters per budget unit (no tokenizer pass, unlike `query_graph`) |
| `detail` | `all` \| `high` | `all` | |

### `explain`

Explain a node: its metadata and up to 20 neighbors with relations and confidence. Errors with `node not found` for unknown labels.

| Argument | Type | Notes |
|---|---|---|
| `node` | string | **required** — node label |

### `get_neighbors`

List a node's neighbors, optionally filtered by relation. Returns the strongest 20 neighbors; relation filtering applies after that cap, so a filtered listing can show fewer neighbors than exist.

| Argument | Type | Notes |
|---|---|---|
| `node` | string | **required** |
| `relation` | string | exact, case-sensitive relation name, e.g. `calls`, `imports`, `uses` |

### `shortest_path`

Shortest path between two nodes, with the relation of each hop.

| Argument | Type | Default | Notes |
|---|---|---|---|
| `source` | string | — | **required** |
| `target` | string | — | **required** |
| `directed` | boolean | `false` | |
| `detail` | `all` \| `high` | `all` | |

### `affected`

Blast radius: everything impacted by changing a node — reverse reachability over the impact relations (`calls`, `references`, `imports`, `imports_from`, `uses`, `depends_on`, `requires`, and `inherits` where a graph carries it). The `relation` argument accepts only these values.

| Argument | Type | Default | Notes |
|---|---|---|---|
| `node` | string | — | **required** |
| `depth` | integer | `2` | |
| `relation` | string | — | Filter to one relation type |

### `god_nodes`

The highest-degree nodes — what everything connects through. No arguments.

### `list_communities`

All communities with labels (hub-based by default; thematic LLM labels when produced by `run --label-communities`), sizes, and cohesion. No arguments.

### `graph_stats`

Node/edge/community/file counts for the graph, plus the graph's modularity when recorded. No arguments. If it reports 0 nodes, the graph has not been built yet — run `astria run <path>` first.

### `health`

Code-health report with unreachable-symbol candidates, circular file dependencies, hub concentration, and graph staleness. Returns a heuristic score from 0 to 100. No arguments.

## Example session

```json
{"jsonrpc": "2.0", "id": 1, "method": "tools/call",
 "params": {"name": "query_graph",
            "arguments": {"question": "where does auth live?", "budget": 1500}}}
```

```text
Traversal: BFS depth=2 | Start: [authenticate_user] | 2 nodes found
NODE authenticate_user [id=src_auth_auth::authenticate_user src=src/auth/auth.rs:45 community=3]
NODE AuthMiddleware [id=src_auth_middleware::authmiddleware src=src/auth/middleware.rs:12 community=3]
EDGE AuthMiddleware --calls [EXTRACTED]--> authenticate_user @src/auth/middleware.rs:28
```

The real output shape: a `Traversal:` header line, `NODE` lines carrying `src=file:line` (plus id and community), and `EDGE` lines carrying the provenance class in brackets (with a calibrated score appended when present, e.g. `[SEMANTIC:0.82]`) and `@file:line`. Truncated results end with a `(continuation: re-run with cursor N …)` footer — see [CLI reference → Query flags](./cli#query-flags) for the same output shape from the command line.

## Fidelity tiers

The tools that expose `detail` (`query_graph`, `repo_map`, `shortest_path` — plus the CLI's `query`/`path`/`map`) accept `detail: "high"` to keep only `EXTRACTED`/`DECLARED` facts — a provenance-class filter, not a numeric threshold — and drop everything inferred, including `learned` edges and `similar_to` embeddings. Use it when an answer must be defensible; use the default `all` for recall.
