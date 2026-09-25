---
sidebar_position: 5
title: Troubleshooting
description: Common issues — install problems, stale graphs, large-graph exports, and when a graph looks wrong.
keywords: [troubleshooting, faq, diagnose, stale graph, native binary]
---

# Troubleshooting

## The graph is empty or `graph_stats` reports 0 nodes

The graph has not been built in that directory. Read-only commands never create a `.astria/` directory — run the pipeline first:

```bash
astria run <path>
```

## Install fails or the native binary is missing

- Node.js **>= 20** is required.
- No Rust toolchain is needed — the native core ships as prebuilt per-platform binaries via the package's optional dependencies. If your npm setup skips optional dependencies (`--no-optional`, an `omit=optional` in `.npmrc`), the binary never downloads; remove that and reinstall.

## Query results look stale

Every `query` output reports when the graph was last built, so you can judge freshness directly. To refresh:

```bash
astria update <path>          # incremental — only changed files
astria watch <path>           # or keep it fresh automatically
```

## `export --format html` refuses on a large repo

That is the safety limit: the default `--mode standard` interactive viewer is capped at 5,000 nodes (the same limit as the original Graphify viewer) and fails with an actionable message beyond that. Explicitly opt into the optimized viewer:

```bash
astria export --graph . --format html --mode large --out graph-view.html
```

## `add --postgres` fails

Postgres introspection shells out to `psql` (read-only over `information_schema` — no credentials are stored). Install the Postgres client and make sure it is on `PATH`.

## First `run --embed` is slow

The one-time ~90 MB local model download. After it, embedding refreshes are incremental and fully offline. To relocate the cache (e.g. onto a persistent dir in CI), set `ASTRIA_EMBED_CACHE_DIR` — see [Environment variables](./env-vars).

## A graph looks wrong

Run `diagnose` first — it is a read-only health report over the existing graph: dangling edge endpoints, self-loops, duplicate edges, unclassified files, and zero-cohesion communities. `--json` for tooling.

```bash
astria diagnose --graph .
```

For noise from fixtures, generated code, or vendored assets, exclude them with a `.astriaignore` file (gitignore syntax) in the project root and rebuild.

## Too much inferred content in answers

Every edge carries a confidence class (`EXTRACTED` / `INFERRED` / `AMBIGUOUS`). Use the high-fidelity tier to see declared facts only:

```bash
astria query "..." --detail high
```

or `detail: "high"` on any MCP traversal tool.

## The token benchmark shows `<1×` on my repo

That is the benchmark being honest, not broken. On tiny corpora, reading the files directly is cheaper than a graph query — there the graph's value is structure (blast radius, communities, paths), not compression. The output says so; see [Benchmarks and evidence](../explanation/benchmarks).

## Learned edges are connecting things I didn't declare

Learned edges are `INFERRED` by design — they record which node pairs your own queries keep connecting. They flow into clustering and exports, but any high-fidelity traversal filters them out. If they mislead, prefer `--detail high` for that session; they are always re-derivable from query history.
