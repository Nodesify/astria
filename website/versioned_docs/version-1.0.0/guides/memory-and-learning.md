---
sidebar_position: 5
title: Memory and learning
description: Two feedback loops — automatic learned edges from query history, and curated Q/A memory via save-result and reflect.
keywords: [memory, learned edges, save-result, reflect, lessons, feedback loop]
---

# Memory and learning

The graph compounds in value as you use it, through two independent loops: **learned edges** (automatic) and **curated memory** (deliberate).

```mermaid
flowchart TD
    A["query / explain / path"] -->|"every run records (seed, discovered) pairs"| B["query history"]
    B -->|"same pair recurs:<br/>2+ distinct questions, 3+ total hits"| C["next run/update promotes it<br/>to a learned edge (INFERRED)"]
    D["save-result"] -->|"Q/A doc with outcome + correction"| E[".astria/memory/"]
    E -->|"ingested as graph nodes on next run/update"| F["graph"]
    E -->|"reflect aggregates"| G["LESSONS.md"]
```

## Learned edges — automatic

Every query records which (seed, discovered) node pairs its traversal connected. When the same pair recurs across **at least 2 distinct questions with 3+ total hits**, the next `run`/`update` promotes it to a `learned` edge — `INFERRED`, hits-scored, provenance `query_history`.

Learned edges flow into clustering, analysis, and every export — the graph remembers which connections you actually keep asking about. High-fidelity traversals (`--detail high`) filter them like any other `INFERRED` fact.

## Curated memory — deliberate

Learned edges are automatic; memory is curated. You decide which answers are worth keeping:

```bash
astria save-result "where is rate limiting?" \
    --answer "BucketMiddleware in src/limiter.rs" \
    --outcome useful \
    --nodes <cited-node-ids>
astria reflect
```

- `save-result` writes a Q/A memory doc (with an outcome — `useful`, `dead_end`, or `corrected` — and optional corrections) into `.astria/memory/`. Cited node ids link the answer back to the graph.
- The next `run`/`update` ingests memory docs as graph nodes, so settled questions become part of the graph itself.
- `reflect` aggregates outcomes into `.astria/reflections/LESSONS.md` with tallies — a running record of which answers held up.

## Why two loops

Learned edges capture *what you ask about* without any ceremony; memory captures *what you settled*, including corrections to earlier wrong answers. Together they mean the graph you have after a month of work is measurably more useful than the one a fresh `run` builds.
