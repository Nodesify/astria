---
sidebar_position: 1
title: Wiki and exports
description: Export the graph as a markdown wiki, Obsidian vault, HTML or SVG visualization, GraphML/JSON, Neo4j, or FalkorDB.
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

The exporter renders a self-contained interactive viewer (no network access needed, works in sandboxed previewers) organized around community bubbles: the graph opens as one bubble per community, sized by member count, with links between bubbles weighted by how many edges connect them. Click a bubble to expand it into its member nodes, click a member node to focus its 1-hop neighborhood — the focus panel names each link (calls, imports, …) and the highlighted edges carry direction arrows — and use the search box to jump to any symbol or community. "All nodes" expands every community at once, with level-of-detail labels that only appear once you zoom in.

The default `--mode standard` accepts graphs of at most 5,000 nodes and fails with an actionable message for larger graphs. For larger repositories, explicitly opt into the large-graph mode:

```bash
astria export --graph . --format html --mode large --out graph-view.html
```

Large mode uses the same community-bubble viewer with no node-count cap. The exported layout is precomputed (no physics) and the viewer draws only what is on screen, so zooming stays responsive even on big graphs — but the page is a single file that embeds the whole graph, so a very large repository (tens of thousands of symbols) produces a proportionally large HTML file that takes longer to open and parse.

The viewer is safe to open on graphs built from untrusted repositories: node and community labels come from repo content (identifiers, docstrings, LLM output) and are rendered strictly as text — never interpolated as HTML — so a crafted label cannot inject script into the exported page.

## GraphML, JSON and Neo4j

```bash
astria export --graph . --out graph.json --format json     # default
astria export --graph . --out graph.graphml --format graphml
astria export --graph . --format cypher --out astria.cypher
cypher-shell -u neo4j -p <password> -f astria.cypher
```

The Neo4j export writes an idempotent import script (`MERGE` statements — safe to re-run). JSON and GraphML exports are unaffected by `--mode`.

### Live Neo4j push

Skip the file entirely and push straight into a running Neo4j over Bolt:

```bash
astria export --graph . --format cypher --neo4j-push bolt://localhost:7687   --neo4j-user neo4j --neo4j-pass <password>   # or NEO4J_USERNAME / NEO4J_PASSWORD env
```

The push is the same idempotent MERGE graph, delivered in parameterized
UNWIND batches (500 rows each): `Symbol` nodes, typed relationships
(`CALLS`, `IMPORTS`, ...), and `Community` nodes carrying labels, summaries,
and `labelSource`. The Bolt client is hand-rolled in Rust (`astria-bolt`):
no driver dependency, mock-server tested.

## SVG

```bash
astria export --graph . --format svg --out graph.svg
```

A standalone dark-theme SVG with a deterministic community-arc layout:
communities sit on a ring, members spread around each center, hub symbols
labeled. No physics, no JS — byte-identical on every run, so it diffs
cleanly and embeds anywhere (GitHub READMEs render it natively). Graphs
beyond 2,000 nodes / 6,000 edges degrade gracefully with a `(capped)` note.

## FalkorDB

```bash
astria export --graph . --format falkordb --out graph.falkordb.cypher
astria export --graph . --format falkordb --redis-push localhost:6379 --graph-name astria
```

The first command writes an idempotent openCypher script with load instructions. The second sends its statements to a running FalkorDB instance through `redis-cli` (which must be on `PATH`).

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
