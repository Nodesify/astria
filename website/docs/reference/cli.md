---
sidebar_position: 1
title: CLI reference
description: Every nodesify-graphify command and flag — building, querying, exporting, memory, the cross-repo global graph, and assistant integration.
keywords: [cli, commands, flags, reference]
---

# CLI reference

All commands accept `--graph .` to point at an existing `.graphify/` directory (defaults to the current directory).

## Building the graph

```bash
nodesify-graphify run <path>                 # Full pipeline: detect → extract → build → cluster → analyze → report
nodesify-graphify run <path> --wiki          # ...also export a markdown wiki to .graphify/wiki
nodesify-graphify run <path> --embed         # ...also compute local embeddings (similar_to edges + semantic query recall)
nodesify-graphify run <path> --global --as <tag>  # ...also merge this repo into the cross-repo global graph (see Global graph)
nodesify-graphify update <path>              # Incremental rebuild (only changed files; regenerates an existing wiki)
nodesify-graphify watch <path> [--debounce 3000]  # Watch for file changes, auto-rebuild
nodesify-graphify cluster-only <path>        # Re-cluster + analyze + report without re-extracting
nodesify-graphify merge <pathA> <pathB> <outPath>  # Merge two graphs
nodesify-graphify diff <pathA> <pathB>       # Compare two graphs
```

Builds also pick up, automatically:

- **Cargo workspaces** — when a `Cargo.toml` is present, workspace members and internal path dependencies become `crate::*` nodes with `crate_depends_on` edges (honoring `package =` renames and `workspace = true` inheritance). No LLM involved — dependency structure is fact.
- **MCP configs** — `.mcp.json`, `mcp_servers.json`, and `claude_desktop_config.json` become `mcp_server`/`mcp_command`/`mcp_package` nodes with `requires_env` edges (env **names only** — values are never read).
- **Transcript sidecars** — any `.txt`/`.md` you drop into `.graphify/transcripts/` is ingested as document nodes on the next run/update. The contract for external transcribers: run any tool you like, write the text there, let the graph index it.

## Querying

```bash
nodesify-graphify explain <node> [--graph .]              # Explain a node and its connections
nodesify-graphify query <question> [--dfs] [--depth 2] [--budget 2000] [--directed] [--detail high] [--cursor N] [--graph .]  # BFS/DFS traversal
nodesify-graphify path <A> <B> [--directed] [--detail high] [--graph .]   # Shortest path between two concepts
nodesify-graphify affected <node> [--depth 2] [--relation R] [--graph .]  # Blast radius - what breaks if you change this node
nodesify-graphify map [--budget 2000] [--graph .]         # PageRank-ranked repo map with top symbols
nodesify-graphify stats [--graph .]                       # Node/edge/community counts
nodesify-graphify status [--graph .]                      # Graph health and staleness
nodesify-graphify history [--limit 20] [--graph .]        # Show recent query history
```

### Query flags {#query-flags}

- `--dfs` — depth-first instead of breadth-first traversal
- `--depth N` — maximum traversal depth
- `--budget N` — output token budget (default 2000)
- `--directed` — follow edge direction instead of treating the graph as undirected
- `--detail high` — fidelity tier: only declared (`EXTRACTED`) facts
- `--cursor N` — continuation cursor for truncated traversals

Query output reports when the graph was last built, so agents can judge freshness. Repeated queries promote recurring node pairs into `learned` edges — see [learning from usage](#learning-from-usage).

### Query log (for tooling)

Every query can also append a JSONL line (ts, kind, question, nodes, duration) to a log file for agent/tooling consumption:

- `GRAPHIFY_QUERY_LOG=<path>` — log to a specific file; `GRAPHIFY_QUERY_LOG=1` uses the default location
- `GRAPHIFY_QUERY_LOG_ENABLE=1` — turn on logging without choosing a path
- `GRAPHIFY_QUERY_LOG_DISABLE=1` — always wins; logging never breaks a query (fails silent)

## Exports and visualization

```bash
nodesify-graphify export [--graph .] [--out graph.json] [--format json|html|graphml|cypher] [--mode standard|large]
nodesify-graphify tree [--out tree.html] [--max-children 40]   # Collapsible filesystem tree of all symbols (HTML)
nodesify-graphify wiki [--out .graphify/wiki] [--max-nodes 25] [--graph .]  # Wikipedia-style markdown wiki
nodesify-graphify prs [20] [--conflicts] [--graph .]           # Map open PRs onto the graph - impact + merge-order risk
```

`export --format html` creates an interactive vis-network graph view. The default `--mode standard` exports the full interactive graph when it contains at most 5,000 nodes and fails with an actionable message for larger graphs. `--mode large` opts into a precomputed-layout viewer (physics-free, key nodes first, batched search) that opens instantly on any repo size.

`--format cypher` writes an idempotent Neo4j import script (MERGE statements — safe to re-run):

```bash
nodesify-graphify export --graph . --format cypher --out graphify.cypher
cypher-shell -u neo4j -p <password> -f graphify.cypher
```

See [Wiki and exports](../guides/wiki-and-exports) for details.

## Graph health

```bash
nodesify-graphify diagnose [--graph .] [--json]
```

Read-only health report over an existing graph: dangling edge endpoints (stub vs actionable), self-loops, duplicate edges, unclassified files, and zero-cohesion communities. `--json` emits machine-readable output. Never mutates the graph.

## Memory and reflection

The feedback loop that complements [learned edges](#learning-from-usage): learned edges are automatic, memory is curated. Full walkthrough in [Memory and learning](../guides/memory-and-learning).

```bash
nodesify-graphify save-result <question> --answer <text> [--answer-file <path>] \
    [--outcome useful|dead_end|corrected] [--correction <text>] [--nodes <ids>] [--graph .]
nodesify-graphify reflect [--graph .]
```

- `save-result` writes a Q/A memory doc (with outcome and corrections) into `.graphify/memory/`. Cited node ids link the answer to the graph.
- The next `run`/`update` ingests memory docs as graph nodes, so settled questions become part of the graph.
- `reflect` aggregates outcomes into `.graphify/reflections/LESSONS.md` with outcome tallies.

## Global graph (cross-repo)

Merge many repo graphs into one queryable store at `~/.nodesify-graphify/global.db` — merging behavior in detail in [Global graph](../guides/global-graph):

```bash
nodesify-graphify run <path> --global --as <tag>   # build, then merge into the global store
nodesify-graphify global add <path> [--as <tag>]   # same merge, standalone (idempotent per tag)
nodesify-graphify global remove <tag>              # prune a repo from the global graph
nodesify-graphify global list                      # registered repos
nodesify-graphify global path <A> <B>              # shortest path across repos
```

Design notes: sourced node ids are prefixed with the repo tag (`<tag>::<id>`); external/stub symbols stay unprefixed and dedupe by label, so `serde_json::Value` means the same thing in every repo. Types sharing `(namespace, label)` across repos get `same_type_as` edges, and parked unresolved calls are resolved when exactly one cross-repo candidate exists (fail closed on ambiguity). Query against the merged store with the usual `--graph` flag pointed at the global db:

```bash
nodesify-graphify query "where is the shared auth type" --graph ~/.nodesify-graphify/global.db
```

## Knowledge ingestion

```bash
nodesify-graphify add <url> [--author] [--contributor]         # Fetch arXiv/tweet/webpage/image/PDF into ./raw + update graph
nodesify-graphify add --scip <index.json>                      # Ingest a simplified SCIP JSON index (rust-analyzer & co.)
nodesify-graphify add --postgres <dsn>                         # Introspect a live PostgreSQL schema (requires psql on PATH)
```

Both `--scip` and `--postgres` are offline/local alternatives to URL fetching: SCIP indexes bring external toolchain symbols into the graph (`scip_impl`/`scip_typed`/`scip_def`/`scip_ref` edges, deterministic ids); Postgres introspection is read-only over `information_schema` (tables/views/routines/FKs → `contains` + `references` edges, no credentials stored). The Postgres DSN is opt-in by flag — nothing calls the network by default.

## Assistant integration

```bash
nodesify-graphify mcp [--graph .]              # Run MCP stdio server - query the graph from any AI agent
nodesify-graphify install [--platform claude]  # Install skill files for AI coding assistants
nodesify-graphify uninstall [--platform claude]  # Uninstall skill files
nodesify-graphify hook install|uninstall|status  # Git hook management
nodesify-graphify hook-guard <mode>            # Editor PreToolUse guard (search | read | gemini) — installed into .claude/settings.json
```

Supported platforms for `install`: `claude`, `codex`, `gemini`, `cursor`, `copilot`, `aider`, `opencode`, `kiro`, `trae`. Setup walkthrough in [Agent integration](../guides/mcp-and-agents); the nine MCP tools are documented in the [MCP tools reference](./mcp-tools).

`install` also injects an always-on `## graphify` instruction block into `AGENTS.md`/`CLAUDE.md` (query before grep, run `update` after edits) — idempotent, removed by `uninstall`. `hook-guard` is the editor-side companion to git hooks: it nudges agents toward `query` before raw searches and can (strict mode, opt-in) gate un-indexed reads. It fails open — any error means the tool call proceeds untouched.

## Learning from usage {#learning-from-usage}

The graph compounds in value as you query it. Every query records which (seed, discovered) node pairs its traversal connected; when the same pair recurs across **at least 2 distinct questions with 3+ total hits**, the next `run`/`update` promotes it to a `learned` edge (`INFERRED`, hits-scored, provenance `query_history`). Learned edges flow into clustering, analysis, and every export — the graph remembers which connections you actually keep asking about. High-fidelity traversals (`--detail high`) can filter them like any `INFERRED` fact.
