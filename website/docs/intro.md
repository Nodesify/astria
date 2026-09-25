---
sidebar_position: 1
title: Introduction
description: astria turns any folder into a queryable knowledge graph — deterministic AST extraction in Rust, optional local embeddings, zero API keys.
keywords: [knowledge graph, codebase, ast, tree-sitter, rust, agents, mcp]
---

# Introduction

**astria** turns any folder into a queryable knowledge graph — deterministic AST extraction in Rust, optional local-embedding semantics, zero API keys, everything on your machine.

You drop into an unfamiliar repo and need to know: what is load-bearing here, what breaks if I change this, where does auth live, how do these two modules connect. Reading everything costs the whole context window. The graph answers in ~3,000 tokens — **measured** at **50–110× fewer tokens per query** on real repos (printed honestly after every run; varies by corpus — see [Benchmarks and evidence](./explanation/benchmarks)).

## Three things a folder full of files can't give you

1. **Structure that survives the session** — hub files, god nodes, communities, and the blast radius of any change, stored in SQLite and refreshed incrementally as code changes.
2. **An honest audit trail** — every edge is labeled `EXTRACTED` / `INFERRED` / `AMBIGUOUS` with a numeric confidence score. You always know what was found in the source versus deduced, and `--detail high` filters to only declared facts.
3. **Answers for agents and humans** — query it from the CLI, from any AI agent via MCP, or just read the exported markdown wiki with plain file links.

## Where to go next

- [Getting started](./getting-started) — install and run your first graph
- [Agent integration](./guides/mcp-and-agents) — wire the graph into Claude Code, Codex, Cursor, …
- [CLI reference](./reference/cli) — every command and flag
- [MCP tools reference](./reference/mcp-tools) — the nine tools your agent can call
- [Wiki and exports](./guides/wiki-and-exports) — markdown wiki, Obsidian vault, HTML viewer, Neo4j
- [Semantic enrichment](./guides/semantic-enrichment) — local embeddings and LLM backends
- [Global graph](./guides/global-graph) — one queryable store across repos
- [Memory and learning](./guides/memory-and-learning) — learned edges and curated Q/A memory
- [Troubleshooting](./reference/troubleshooting) — when something looks wrong
- [Benchmarks and evidence](./explanation/benchmarks) — measured numbers and the head-to-head vs the original Graphify
- [Architecture](./explanation/architecture) — how the pipeline works under the hood

Worked examples with honest reviews — the tool run on itself and on its Python ancestor, including what the graph got *wrong* — live in the [`worked/`](https://github.com/Nodesify/astria/tree/main/worked) directory of the repository, alongside a [head-to-head comparison](https://github.com/Nodesify/astria/tree/main/worked/head-to-head) against the original Python Graphify on the same corpus.
