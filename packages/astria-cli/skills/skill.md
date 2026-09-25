---
name: graphify
description: Turn any directory into a queryable knowledge graph. Trigger: /graphify
---

# /graphify

Turn any directory of source code into a queryable knowledge graph with community detection, hub node analysis, and a plain-language graph report. Uses AST-based extraction via tree-sitter for deterministic, fast analysis.

## What You Must Do When Invoked

If no path was given, use `.` (current directory). Do not ask the user for a path.

Follow these steps in order. Do not skip steps.

### Step 1 - Check graph state and build if needed

```bash
node -e "const fs=require('fs');const p='.graphify/graph.json';if(!fs.existsSync(p)){console.log('missing');process.exit(0)}const age=Math.round((Date.now()-fs.statSync(p).mtimeMs)/60000);console.log(age>30?'stale':'fresh')"
```

Act on the result:
- `missing`: Run `nodesify-graphify run .` — this builds the full graph from scratch. Wait for it to complete.
- `stale`: Run `nodesify-graphify update .` — incremental rebuild of changed files. Wait for it to complete.
- `fresh`: Skip to Step 2.

After running `run` or `update`, verify with `nodesify-graphify stats --graph .` that Nodes > 0 (an incremental update with no changes correctly reports 0 nodes added). If Nodes is 0, tell the user: "No supported files found. graphify supports Python, JS, TS, Rust, Go, Java, C, C++, Ruby, Swift, Scala, PHP, C#, Lua, Haskell, Elixir, Bash, Dart, Zig, CSS, Markdown, plain text, and PDF."

### Step 2 - Read the graph report

Read `.graphify/graph_report.md`. Present a clean summary:

```
Knowledge graph built: X nodes, Y edges, Z communities

Hub nodes (most connected):
  1. NodeName — N connections (community C)
  2. ...

Surprising connections:
  - SourceNode → TargetNode [cross-community edge]

Suggested questions:
  - "Question 1?"
  - "Question 2?"
```

Do NOT dump the raw report. Summarize it.

### Step 3 - Ready for queries

Tell the user the graph is ready and they can ask questions. Example queries they might try:
- `nodesify-graphify query "authentication flow"`
- `nodesify-graphify path "AuthService" "UserModel"`
- `nodesify-graphify explain "validate_token"`

## Enforcement Rules

When working in a project with a `.graphify/` directory, these rules are MANDATORY:

### FORBIDDEN

Do NOT use `Grep`, `Glob`, or file browsing as your first step for architecture or codebase questions. This includes:
- Searching for "where is X defined"
- Looking for "how does X connect to Y"
- Exploring "what files are in this project"
- Finding "all files that reference X"

### REQUIRED

Instead, use these graphify commands:

| User asks | Use this command |
|-----------|-----------------|
| "Where is X implemented?" | `nodesify-graphify query "X"` |
| "How does X connect to Y?" | `nodesify-graphify path "X" "Y"` |
| "What does X do?" | `nodesify-graphify explain "X"` |
| "What is the architecture?" | Read `.graphify/wiki/index.md` if present, else `.graphify/graph_report.md` |
| "Give me a navigable overview" | `nodesify-graphify wiki --graph .` then read `.graphify/wiki/index.md` |
| "Find all references to X" | `nodesify-graphify query "X" --depth 3` |
| "What community is X in?" | `nodesify-graphify explain "X"` |

Only use native file reading AFTER the graph has identified the exact files you need.

### When native tools ARE appropriate

- Editing a specific file (after graph identified which file)
- Reading a file the user explicitly named
- Running tests or build commands
- Git operations

### When direct search / file reading is better than the graph

The graph models entities and relationships — it does NOT model behavior.
Use Grep/Glob/file reads first for:

- **Predicate-level bugs**: "is this window check off by one?", "does this
  regex match branch slugs?" — expression semantics are invisible to a
  graph. Read the code.
- **Exact string audits**: checking every occurrence of a literal in a
  handful of files, or auditing env var names and CLI flags. Grep is
  deterministic and fast here.
- **Natural-language discovery**: the graph anchors on symbol names.
  `query` handles typos and partial matches, but if you don't know any
  symbol name yet, a broad grep for a distinctive string is often faster.

Rule of thumb: use the graph to identify WHICH files matter (blast radius,
architecture, cross-module dependencies), then read those files directly
for exact logic. Graph output includes `file:line` anchors precisely so
you can jump straight from a node or edge to the source.

### Provenance and reference nodes

- Every `NODE` line in `query` output carries `src=path:line`, and every
  `EDGE` line ends with `@path:line` — the exact spot the relationship was
  extracted from. `explain` prints `File: path:line` for the node and each
  neighbor.
- Identifier-shaped string literals (env vars like `PLANE_URL`, snake_case
  keys like `needs_human`, dotted/kebab/slash chains like
  `harness/hr-101-fix-redis-leak`) are indexed as global `reference` nodes
  with `references` edges — so "where is this config key / status value
  used?" is a graph query, not a grep. Query output ends with a
  `# graph built at <timestamp>` line so you can judge freshness.

## Command Reference

### `nodesify-graphify run <path>`

Full pipeline: detect → extract → build → cluster → analyze → report.

Creates `.graphify/` with `db.sqlite`, `graph.json`, `graph_report.md`.

### `nodesify-graphify update <path>`

Incremental rebuild — only re-extracts files that changed (SHA-256 detection).

Much faster than `run` for existing projects.

### `nodesify-graphify query <question> [options]`

BFS (default) or DFS graph traversal from nodes matching your question.

```
nodesify-graphify query "authentication"              # BFS, depth 2, budget 2000
nodesify-graphify query "database" --dfs --depth 3    # DFS, deeper
nodesify-graphify query "error handling" --budget 3000 # more output
```

Options:
- `--dfs` — depth-first search (traces specific paths)
- `--depth <n>` — traversal depth (default: 2)
- `--budget <n>` — token budget for output (default: 2000)
- `--directed` — follow edges only in their stored direction (caller -> callee, importer -> module)
- `--detail high` — keep only EXTRACTED/DECLARED facts, dropping inferred/semantic edges
- `--cursor <n>` — continuation token from a previous truncated query
- `--graph <path>` — project root (default: `.`)

### `nodesify-graphify explain <node> [options]`

Show a node's details and all its connections.

```
nodesify-graphify explain "UserService"
```

### `nodesify-graphify path <A> <B> [options]`

Find shortest path between two concepts.

```
nodesify-graphify path "AuthService" "Database"
nodesify-graphify path "AuthService" "Database" --directed   # only caller -> callee direction
```

### `nodesify-graphify affected <node> [options]`

Blast radius — everything impacted by changing a node (reverse reachability over calls/imports/uses).

```
nodesify-graphify affected "UserService" --depth 3
nodesify-graphify affected "UserService" --relation calls
```

### `nodesify-graphify map [options]`

Aider-style repo map: files ranked by PageRank over the reference graph, with each file's most-connected symbols. Best first command when orienting on a codebase.

```
nodesify-graphify map --budget 2000
```

### Semantic layer (local embeddings)

When the graph was built with `run --embed`, `query` automatically merges semantic recall with token matching — conceptual questions with zero string overlap still find their symbols, and `similar_to` edges link related concepts across files. No API key; the model is downloaded once and cached (`GRAPHIFY_EMBED_CACHE_DIR` overrides the location).

### Learning from usage

Repeated queries leave a trace: node pairs that keep coming up across distinct questions are promoted to `learned` edges on the next `run`/`update`. The graph gets better connected in exactly the places the codebase is actually explored.

### MCP server

`nodesify-graphify mcp --graph .` runs an MCP stdio server exposing the graph to AI agents with tools: `query_graph` (supports `cursor` continuation and `detail` tiers), `repo_map`, `explain`, `get_neighbors`, `shortest_path`, `affected`, `god_nodes`, `list_communities`, `graph_stats`.


### `nodesify-graphify stats [options]`

Quick graph health check: node count, edge count, communities, files.

### `nodesify-graphify status [options]`

Graph staleness check: fresh/stale/very_stale with age in minutes.

### `nodesify-graphify export [options]`

Export graph to JSON, HTML, GraphML, or Cypher (Neo4j).

```
nodesify-graphify export --format html --out graph.html
nodesify-graphify export --format graphml --out graph.graphml
nodesify-graphify export --format cypher --out graphify.cypher   # idempotent MERGE script for Neo4j
```

### `nodesify-graphify wiki [options]`

Wikipedia-style markdown wiki of the graph: `index.md` plus one article per community and per god node, cross-linked with relative markdown links. Readable by any agent without the CLI — point an agent at `.graphify/wiki/index.md` and it navigates by reading files.

```
nodesify-graphify wiki --graph .              # writes .graphify/wiki/
nodesify-graphify wiki --out docs/wiki        # e.g. for GitHub
nodesify-graphify wiki --format obsidian --out my-vault   # Obsidian vault + canvas
```

## Post-Edit Protocol

After modifying code files in a session with an active graph:

```bash
nodesify-graphify update .
```

Or start a watcher at session beginning:

```bash
nodesify-graphify watch . --debounce 3000
```

This keeps the graph current so subsequent queries reflect your changes.

## Troubleshooting

**"Graph is empty (0 nodes)"**
- Run `nodesify-graphify run .` to build from scratch
- Check that the directory has supported file types

**"Query returns no results"**
- Try different search terms (partial matches work)
- Use broader terms: "auth" instead of "authenticateUserWithOAuth"
- Check `nodesify-graphify stats` to verify graph has data

**"Graph seems stale"**
- Run `nodesify-graphify update .` for incremental refresh
- Or `nodesify-graphify run .` for full rebuild

**"Status says stale"**
- The `graph.json` was built more than 30 minutes ago
- Run `nodesify-graphify update .` before querying
