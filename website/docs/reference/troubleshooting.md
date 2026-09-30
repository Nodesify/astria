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

- Node.js **>= 22** is required.
- No Rust toolchain is needed — the native core ships as prebuilt per-platform binaries via the package's optional dependencies. If your npm setup skips optional dependencies (`--no-optional`, an `omit=optional` in `.npmrc`), the binary never downloads; remove that and reinstall.

## Query results look stale

Every `query` output reports when the graph was last built, so you can judge freshness directly. To refresh:

```bash
astria update <path>          # incremental — only changed files
astria watch <path>           # or keep it fresh automatically
astria hook install           # or refresh quietly after every commit (throttled)
```

## A query returns "No confident match"

The no-confident-match guard fired: none of the question's key (highest-IDF) terms appear anywhere in the graph, and no node matched even half of the question's terms — so any traversal would be seeded by an incidental word match ("handled" fuzzy-matching `handle_message()`) rather than an answer. The message names the terms it could not find.

That is the honest answer when the topic genuinely is not in the corpus (a payroll question against a repo with no payroll code). To get results anyway:

- Rephrase toward the code's own vocabulary — a symbol name, a file path, or the words a docstring would use.
- If a concept is present but shares no vocabulary with your question, build with `--embed` so semantic recall can bridge the gap (a qualifying embedding candidate bypasses the guard).
- To restore the historical always-traverse behavior for a run (e.g. measuring IR-style recall), set `ASTRIA_QUERY_SEED_FLOOR=off`. See [environment variables](./env-vars.md).

## `export --format html` refuses on a large repo

That is the safety limit: the default `--mode standard` interactive viewer is capped at 5,000 nodes and fails with an actionable message beyond that. Explicitly opt into the optimized viewer:

```bash
astria export --graph . --format html --mode large --out graph-view.html
```

## `add --postgres` fails

Postgres introspection shells out to `psql` (read-only over the pg system catalogs — no credentials are stored). Install the Postgres client and make sure it is on `PATH`.

## First `run --embed` is slow

The one-time ~615 MB local model download (jina-embeddings-v2-base-code, ONNX fp32). After it, embedding refreshes are incremental and fully offline. To relocate the cache (e.g. onto a persistent dir in CI), set `ASTRIA_EMBED_CACHE_DIR` — see [Environment variables](./env-vars).

## A graph looks wrong

Run `diagnose` first — it is a read-only health report over the existing graph: dangling edge endpoints, self-loops, duplicate edges, unclassified files, and zero-cohesion communities. `--json` for tooling.

```bash
astria diagnose --graph .
```

For noise from fixtures, generated code, or vendored assets, exclude them with a `.astriaignore` file (gitignore syntax) in the project root and rebuild.

## Too much inferred content in answers

Every edge carries a confidence class (`EXTRACTED` / `INFERRED` / `SEMANTIC` / `AMBIGUOUS` — LLM enrichment edges are `SEMANTIC`). Use the high-fidelity tier to see declared facts only:

```bash
astria query "..." --detail high
```

or `detail: "high"` on any MCP traversal tool.

## The token benchmark shows `<1×` on my repo

That is the benchmark being honest, not broken. On tiny corpora, reading the files directly is cheaper than a graph query — there the graph's value is structure (blast radius, communities, paths), not compression. The output says so; see [Benchmarks and evidence](../explanation/benchmarks).

## The retrieval-quality numbers look low

They are measured, not estimated: the harness in `scripts/bench/quality/`
asks grounded questions through the real engine and scores whether the
expected file or symbol appears in the answer. Low recall@k usually means
the answer is *topically* right but surfaces hub nodes and doc headings
before the implementing code — a ranking problem, not an extraction one.
Re-run with `--embed` (semantic seeds change the candidate set), and prefer
`--detail high` to see declared facts only. The harness README documents
the miss mode and the ranking fixes in flight.

## Learned edges are connecting things I didn't declare

Learned edges are `INFERRED` by design — they record which node pairs your own queries keep connecting. They flow into clustering and exports, but any high-fidelity traversal filters them out. If they mislead, prefer `--detail high` for that session; they are always re-derivable from query history.
