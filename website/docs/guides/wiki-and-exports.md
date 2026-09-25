---
sidebar_position: 4
title: Wiki and exports
---

# Wiki and exports

## Markdown wiki

`nodesify-graphify wiki` writes a Wikipedia-style markdown wiki into `.graphify/wiki/`: an `index.md` entry point, one article per community (key concepts ranked by connections, cross-community links, source files, `EXTRACTED`/`INFERRED`/`AMBIGUOUS` audit trail), and one article per god node (signature, connections grouped by relation). Articles cross-link with relative markdown links, so any agent — or GitHub, or Obsidian — can navigate the graph by reading files instead of running queries:

```bash
nodesify-graphify run . --wiki          # build graph + wiki in one step
nodesify-graphify wiki --graph .        # (re)generate the wiki any time
nodesify-graphify wiki --out docs/wiki  # export into docs/ for GitHub
```

`update` regenerates an existing wiki automatically, so it never drifts stale.

## Obsidian vault

`--format obsidian` writes an Obsidian vault instead: one note per node with `graphify/*` + community tags and `[[wikilinks]]` to neighbors, `_COMMUNITY_*.md` overview notes, and a `graphify.canvas` (communities as colored groups, nodes as cards). Open the output directory as a vault in Obsidian:

```bash
nodesify-graphify wiki --format obsidian --out my-vault
```

On this repository the vault produced 2,040 notes and 5,000 canvas edges.

## HTML visualization

```bash
nodesify-graphify export --graph . --format html --out graph-view.html
```

The default `--mode standard` exports the full interactive vis-network graph when it contains at most 5,000 nodes (the same safety limit as the original Graphify viewer) and fails with an actionable message for larger graphs. For larger repositories, explicitly opt into the optimized large-graph viewer:

```bash
nodesify-graphify export --graph . --format html --mode large --out graph-view.html
```

Large mode precomputes node positions, disables physics, shows the highest-degree nodes first, supports debounced search and a "Show all nodes" toggle, caps the community legend, and disables expensive edge arrows for very large graphs.

## GraphML, JSON and Neo4j

```bash
nodesify-graphify export --graph . --out graph.json --format json     # default
nodesify-graphify export --graph . --out graph.graphml --format graphml
nodesify-graphify export --graph . --format cypher --out graphify.cypher
cypher-shell -u neo4j -p <password> -f graphify.cypher
```

The Neo4j export writes an idempotent import script (`MERGE` statements — safe to re-run). JSON and GraphML exports are unaffected by `--mode`.

## Hyperedges in exports

Hyperedges are n-ary node groups produced deterministically at build time (no LLM): one per community (`participate_in`, top-degree members) and one per identifier-shaped literal referenced from ≥ 3 distinct files (`shares_reference`). Every export surface shows them:

- `graph.json` carries a `hyperedges` array (shape-compatible with the official Graphify consumer)
- the HTML viewer shades a convex hull over each hyperedge's member nodes (large mode draws labeled circles)
- the wiki index lists them; `explain` shows a node's hyperedge memberships
- the `GRAPH_REPORT.md` gains a hyperedge section

## PR impact analysis

```bash
nodesify-graphify prs [20] [--conflicts]
```

Maps open pull requests onto the graph: which communities they touch, what their blast radius is, and a merge-order risk ranking.
