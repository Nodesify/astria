---
sidebar_position: 1
title: Wiki and exports
description: Export the graph as an agent-crawlable markdown wiki, an Obsidian vault, an interactive HTML viewer, or GraphML/JSON/Neo4j.
keywords: [wiki, obsidian, export, html, graphml, neo4j, cypher, visualization]
---

# Wiki and exports

## Markdown wiki

`astria wiki` writes a Wikipedia-style markdown wiki into `.astria/wiki/`: an `index.md` entry point, one article per community (key concepts ranked by connections, cross-community links, source files, `EXTRACTED`/`INFERRED`/`AMBIGUOUS` audit trail), and one article per god node (signature, connections grouped by relation). Articles cross-link with relative markdown links, so any agent — or GitHub, or Obsidian — can navigate the graph by reading files instead of running queries:

```bash
astria run . --wiki          # build graph + wiki in one step
astria wiki --graph .        # (re)generate the wiki any time
astria wiki --out docs/wiki  # export into docs/ for GitHub
```

`update` regenerates an existing wiki automatically, so it never drifts stale.

## Obsidian vault

`--format obsidian` writes an Obsidian vault instead: one note per node with `astria/*` + community tags and `[[wikilinks]]` to neighbors, `_COMMUNITY_*.md` overview notes, and a `astria.canvas` (communities as colored groups, nodes as cards). Open the output directory as a vault in Obsidian:

```bash
astria wiki --format obsidian --out my-vault
```

On this repository the vault produced 2,040 notes and 5,000 canvas edges.

## HTML visualization

```bash
astria export --graph . --format html --out graph-view.html
```

The default `--mode standard` exports the full interactive vis-network graph when it contains at most 5,000 nodes and fails with an actionable message for larger graphs. For larger repositories, explicitly opt into the optimized large-graph viewer:

```bash
astria export --graph . --format html --mode large --out graph-view.html
```

Large mode precomputes node positions, disables physics, shows the highest-degree nodes first, supports debounced search and a "Show all nodes" toggle, caps the community legend, and disables expensive edge arrows for very large graphs.

The viewer is safe to open on graphs built from untrusted repositories: node and community labels come from repo content (identifiers, docstrings, LLM output) and are rendered strictly as text — never interpolated as HTML — so a crafted label cannot inject script into the exported page.

## GraphML, JSON and Neo4j

```bash
astria export --graph . --out graph.json --format json     # default
astria export --graph . --out graph.graphml --format graphml
astria export --graph . --format cypher --out astria.cypher
cypher-shell -u neo4j -p <password> -f astria.cypher
```

The Neo4j export writes an idempotent import script (`MERGE` statements — safe to re-run). JSON and GraphML exports are unaffected by `--mode`.

## Hyperedges in exports

Hyperedges are n-ary node groups produced deterministically at build time (no LLM): one per community (`participate_in`, top-degree members) and one per identifier-shaped literal referenced from ≥ 3 distinct files (`shares_reference`). Every export surface shows them:

- `graph.json` carries a `hyperedges` array (shape-compatible with Graphify's consumer)
- the HTML viewer shades a convex hull over each hyperedge's member nodes (large mode draws labeled circles)
- the wiki index lists them; `explain` shows a node's hyperedge memberships
- the `GRAPH_REPORT.md` gains a hyperedge section

## PR impact analysis

```bash
astria prs [20] [--conflicts]
```

Maps open pull requests onto the graph: which communities they touch, what their blast radius is, and a merge-order risk ranking.
