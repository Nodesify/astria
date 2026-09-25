---
slug: nodesify-graphify-0-8-0
title: "nodesify-graphify 0.8.0: markdown wiki, Obsidian vault, local embeddings"
authors: [nodesify]
tags: [release]
---

**0.8.0** is out — `npm install -g @nodesify/graphify`. The graph now exports itself as documentation, adds a local semantic layer, and keeps learning from how you use it.

<!-- truncate -->

Highlights:

- **Markdown wiki export** — `wiki` / `run --wiki`: an agent-crawlable wiki (`index.md` + one article per community and god node, relative markdown links that GitHub and Obsidian both navigate). `update` regenerates it, so it never drifts stale.
- **Obsidian vault export** — `wiki --format obsidian`: per-node notes with frontmatter tags and `[[wikilinks]]`, community overviews, and a `graphify.canvas`. On this repo: 2,040 notes, 5,000 canvas edges.
- **Local semantic layer** — `run --embed` downloads a small local model once (~90 MB, then offline forever — no API key) and adds `similar_to` edges plus embedding-backed query recall. Communities on this repo consolidated 401 → 194.
- **Learning from usage** — repeated queries promote recurring node pairs into `learned` edges; the graph compounds in value the more you use it.
- **Neo4j export** — `export --format cypher` writes an idempotent `MERGE` script for `cypher-shell`.
- **Token benchmark** — every run prints a measured corpus-vs-query token comparison (110× on this repo), whether you like the number or not.
- **Security** — esbuild advisory pinned out, Windows reserved-name guards in exports, learned-edge promotion hardened against stale node references.

The unreleased `Next` docs version already covers what is landing after 0.8.0: the deterministic hypergraph, the cross-repo global graph, graph health diagnostics, and offline-first ingest (SCIP indexes, Postgres introspection, MCP configs). See the [docs](/docs/intro) — and [benchmarks](/docs/explanation/benchmarks) for how every number above is measured and reproduced.
