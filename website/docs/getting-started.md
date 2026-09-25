---
sidebar_position: 2
title: Getting started
description: Install @nodesify/graphify, build your first knowledge graph, query it, and keep it fresh — in under five minutes.
keywords: [install, npm, getting started, quickstart, graphify]
---

# Getting started

## Install

```bash
npm install -g @nodesify/graphify
```

Requires no Rust toolchain — ships prebuilt native binaries via napi-rs. Node.js >= 20.

## Build your first graph

Drop into any project folder and run:

```bash
nodesify-graphify run .
```

This runs the full pipeline — detect → extract → build → cluster → analyze → report — and creates a `.graphify/` directory containing:

- `db.sqlite` — the graph database
- `graph.json` — full graph export
- `graph_report.md` — report with hub nodes, communities, surprising connections

Combine building with a wiki export or embeddings in one step:

```bash
nodesify-graphify run . --wiki    # ...also export a markdown wiki to .graphify/wiki
nodesify-graphify run . --embed   # ...also compute local embeddings (similar_to edges + semantic query recall)
```

## Keep it fresh

```bash
nodesify-graphify update <path>                    # Incremental rebuild (only changed files; regenerates an existing wiki)
nodesify-graphify watch <path> [--debounce 3000]   # Watch for file changes, auto-rebuild
```

`update` only re-extracts files that changed since the last run, so large repos rebuild in seconds.

## Query it

```bash
nodesify-graphify explain <node>                  # Explain a node and its connections
nodesify-graphify query "where does auth live"    # BFS traversal with a token budget
nodesify-graphify path <A> <B>                    # Shortest path between two concepts
nodesify-graphify affected <node>                 # Blast radius - what breaks if you change this node
nodesify-graphify map                             # PageRank-ranked repo map with top symbols
```

See the [CLI reference](./reference/cli) for every command and flag.

## Use it from AI agents

```bash
nodesify-graphify mcp          # Run MCP stdio server - query the graph from any AI agent
nodesify-graphify install      # Install skill files for AI coding assistants
```

`mcp` exposes the graph over the Model Context Protocol, so any MCP-capable agent (Claude Code, Codex, Cursor, …) can query it — see the [MCP tools reference](./reference/mcp-tools) for the tool list. `install` writes skill files for your assistant of choice; the full setup (platforms, git hooks, the editor guard) is on [Agent integration](./guides/mcp-and-agents).

Supported platforms: `claude`, `codex`, `gemini`, `cursor`, `copilot`, `aider`, `opencode`, `kiro`, `trae`.

Git hooks can keep the graph fresh automatically:

```bash
nodesify-graphify hook install|uninstall|status
```

## Health, memory, and many repos at once

Three more loops worth knowing about (full flags in the [CLI reference](./reference/cli)):

```bash
nodesify-graphify diagnose                        # read-only graph health report (--json for tooling)
nodesify-graphify save-result "Q" --answer "A"    # curate a settled Q/A into graph memory
nodesify-graphify reflect                         # aggregate memory outcomes into LESSONS.md
nodesify-graphify run . --global --as myrepo      # merge this repo into the cross-repo global graph
```

`diagnose` is the first stop when a graph looks wrong (see [Troubleshooting](./reference/troubleshooting)). The memory loop (`save-result` → `update` → `reflect`) turns settled questions into graph nodes — curated, on top of the automatic learned edges (see [Memory and learning](./guides/memory-and-learning)). And the [global graph](./guides/global-graph) merges many repos into one queryable store, unifying shared external symbols across repos.

## Excluding files

Place a `.graphifyignore` file in your project root (gitignore syntax) to exclude files from the graph.

## Where the token savings come from

Every `run` and `update` prints an honest cost measurement: corpus tokens (the real file sizes from the manifest) versus the tokens a graph query actually returns, sampled over five representative questions. On this repository at v0.8.0: ~333,000 corpus tokens vs ~3,000 per query — **110× fewer tokens per query**; on the original Python Graphify's codebase: **52×**. On tiny corpora it will honestly report &lt;1×; there the graph's value is structure, not compression, and the output says so.

Numbers vary per run and per corpus — the full methodology, a head-to-head against the original Python Graphify, and the embedding experiment are on the [Benchmarks and evidence](./explanation/benchmarks) page.
