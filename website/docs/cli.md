---
sidebar_position: 3
title: CLI reference
---

# CLI reference

All commands accept `--graph .` to point at an existing `.graphify/` directory (defaults to the current directory).

## Building the graph

```bash
nodesify-graphify run <path>                 # Full pipeline: detect → extract → build → cluster → analyze → report
nodesify-graphify run <path> --wiki          # ...also export a markdown wiki to .graphify/wiki
nodesify-graphify run <path> --embed         # ...also compute local embeddings (similar_to edges + semantic query recall)
nodesify-graphify update <path>              # Incremental rebuild (only changed files; regenerates an existing wiki)
nodesify-graphify watch <path> [--debounce 3000]  # Watch for file changes, auto-rebuild
nodesify-graphify cluster-only <path>        # Re-cluster + analyze + report without re-extracting
nodesify-graphify merge <pathA> <pathB> <outPath>  # Merge two graphs
nodesify-graphify diff <pathA> <pathB>       # Compare two graphs
```

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

### Query flags

- `--dfs` — depth-first instead of breadth-first traversal
- `--depth N` — maximum traversal depth
- `--budget N` — output token budget (default 2000)
- `--directed` — follow edge direction instead of treating the graph as undirected
- `--detail high` — fidelity tier: only declared (`EXTRACTED`) facts
- `--cursor N` — continuation cursor for truncated traversals

Query output reports when the graph was last built, so agents can judge freshness. Repeated queries promote recurring node pairs into `learned` edges — see [learning from usage](#learning-from-usage).

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

See [Wiki and exports](./wiki-and-exports) for details.

## Knowledge ingestion

```bash
nodesify-graphify add <url> [--author] [--contributor]  # Fetch arXiv/tweet/webpage/image/PDF into ./raw + update graph
```

## Assistant integration

```bash
nodesify-graphify mcp [--graph .]              # Run MCP stdio server - query the graph from any AI agent
nodesify-graphify install [--platform claude]  # Install skill files for AI coding assistants
nodesify-graphify uninstall [--platform claude]  # Uninstall skill files
nodesify-graphify hook install|uninstall|status  # Git hook management
```

Supported platforms for `install`: `claude`, `codex`, `gemini`, `cursor`, `copilot`, `aider`, `opencode`, `kiro`, `trae`.

## Learning from usage

The graph compounds in value as you query it. Every query records which (seed, discovered) node pairs its traversal connected; when the same pair recurs across **at least 2 distinct questions with 3+ total hits**, the next `run`/`update` promotes it to a `learned` edge (`INFERRED`, hits-scored, provenance `query_history`). Learned edges flow into clustering, analysis, and every export — the graph remembers which connections you actually keep asking about. High-fidelity traversals (`--detail high`) can filter them like any `INFERRED` fact.
