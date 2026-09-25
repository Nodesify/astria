---
sidebar_position: 2
title: MCP tools reference
description: The nine tools exposed by the nodesify-graphify MCP stdio server, with arguments, defaults, and example calls.
keywords: [mcp, tools, query_graph, repo_map, explain, affected, model context protocol]
---

# MCP tools reference

`nodesify-graphify mcp` runs an MCP stdio server (newline-delimited JSON-RPC 2.0, protocol `2025-06-18`, server name `nodesify-graphify`). Any MCP-capable agent — Claude Code, Codex, Cursor, … — can point at it and query the graph without shelling out to the CLI.

Point your agent's MCP config at it:

```json
{
  "mcpServers": {
    "graphify": {
      "command": "nodesify-graphify",
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
| `budget` | integer | `2000` | Output token budget |
| `directed` | boolean | `false` | Follow edges only in stored direction (caller → callee, importer → module) |
| `detail` | `all` \| `high` | `all` | `high` keeps only `EXTRACTED` facts, dropping inferred and semantic edges |
| `cursor` | integer | `0` | Continuation token from a previous truncated result |

Truncated results report the next cursor value — re-run with `cursor` set to fetch the next slice.

### `repo_map`

Aider-style repo map: files ranked by PageRank over the reference graph, with top symbols per file. One budgeted blob to orient on a codebase.

| Argument | Type | Default | Notes |
|---|---|---|---|
| `budget` | integer | `2000` | |
| `detail` | `all` \| `high` | `all` | |

### `explain`

Explain a node: its metadata and up to 20 neighbors with relations and confidence. Errors with `node not found` for unknown labels.

| Argument | Type | Notes |
|---|---|---|
| `node` | string | **required** — node label |

### `get_neighbors`

List a node's neighbors, optionally filtered by relation.

| Argument | Type | Notes |
|---|---|---|
| `node` | string | **required** |
| `relation` | string | e.g. `Calls`, `Imports`, `Uses` |

### `shortest_path`

Shortest path between two nodes, with the relation of each hop.

| Argument | Type | Default | Notes |
|---|---|---|---|
| `source` | string | — | **required** |
| `target` | string | — | **required** |
| `directed` | boolean | `false` | |
| `detail` | `all` \| `high` | `all` | |

### `affected`

Blast radius: everything impacted by changing a node — reverse reachability over calls/imports/uses.

| Argument | Type | Default | Notes |
|---|---|---|---|
| `node` | string | — | **required** |
| `depth` | integer | `2` | |
| `relation` | string | — | Filter to one relation type |

### `god_nodes`

The highest-degree nodes — what everything connects through. No arguments.

### `list_communities`

All communities with their hub-based labels, sizes, and cohesion. No arguments.

### `graph_stats`

Node/edge/community/file counts for the graph. No arguments. If it reports 0 nodes, the graph has not been built yet — run `nodesify-graphify run <path>` first.

## Example session

```json
{"jsonrpc": "2.0", "id": 1, "method": "tools/call",
 "params": {"name": "query_graph",
            "arguments": {"question": "where does auth live?", "budget": 1500}}}
```

```text
NODE  authenticate_user  src/auth/auth.rs:45
NODE  AuthMiddleware     src/auth/middleware.rs:12
EDGE  AuthMiddleware ─CALLS→ authenticate_user  EXTRACTED · 0.97  @middleware.rs:28

(2 nodes, 1 edge)
```

Every `EDGE` line carries its provenance (`@file:line`) and confidence class, and node lines carry `src=file:line` — see [CLI reference → Query flags](./cli#query-flags) for the same output shape from the command line.

## Fidelity tiers

All traversal tools accept `detail: "high"` to keep only declared facts (`EXTRACTED`, strength ≥ 0.9) and drop everything inferred — including `learned` edges and `similar_to` embeddings. Use it when an answer must be defensible; use the default `all` for recall.
