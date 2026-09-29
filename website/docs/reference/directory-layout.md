---
sidebar_position: 7
title: The .astria directory
description: Every file astria writes — the SQLite graph, exports, report, wiki, memory, reflections, transcripts, downloads, and the cross-repo global store.
keywords: [.astria, directory, layout, db.sqlite, graph.json, global.db, gitignore]
---

# The .astria directory

Everything astria produces lives in plain files, so it is inspectable, backup-able, and disposable. Running `astria run <path>` creates:

```
<repo>/
  .astriaignore              # (optional, repo root) gitignore-syntax exclusion list
  raw/                       # (after `add <url>`) downloaded papers/pages/images
  .astria/
    db.sqlite                # The graph database — the source of truth
    db.sqlite-wal / -shm     # SQLite WAL sidecars while a process has the database open
    graph.json               # Full graph export (nodes, edges, hyperedges, communities)
    graph_report.md          # Markdown report: hub nodes, communities, surprising connections
    cache/last_query_stamp   # Query-freshness stamp, rewritten after each query/explain/path
    wiki/                    # (after `wiki` / `run --wiki`) markdown wiki: index.md + articles
    memory/                  # (after `save-result`) curated Q/A docs (also inserted into the graph at save time)
    reflections/             # (after `reflect`) LESSONS.md outcome aggregation
    transcripts/             # (optional) drop .txt/.md here; ingested as document nodes
```

Plus, outside the repo:

```
~/.astria/global.db          # Cross-repo global graph store (`global add`, `run --global`)
~/.astria-embed-cache/       # Local embedding model (~90 MB, downloaded once; override with ASTRIA_EMBED_CACHE_DIR)
```

## Inside db.sqlite

The database is plain SQLite — open it with any SQLite client. Tables:

| Table | Contents |
|---|---|
| `nodes` / `edges` | The graph (see [Graph model](./graph-model)) |
| `hyperedges` | N-ary groups (`participate_in`, `shares_reference`) |
| `communities` | Cluster assignments, labels, cohesion |
| `file_manifest` | Every detected file and its hash — powers incremental `update` |
| `extraction_cache` | Per-file extraction results, keyed by content hash — unchanged files are not re-extracted |
| `pipeline_runs` | Run history: timing, status, files/nodes/edges counters, and LLM usage (`llm_*` columns) |
| `query_history` / `query_pairs` | Query log and the (seed, discovered) pairs that feed `learned` edge promotion |
| `node_embeddings` | Vectors from `run --embed` |
| `_meta` | Schema version and build metadata (what `status` reads) |

## Version control

`.astria/` is safe to gitignore (the default posture — rebuild any time with `astria run .`), but nothing stops you from committing parts of it: the `wiki/` export is plain markdown that renders on GitHub, and `graph_report.md` reads well as repo documentation. `raw/` (downloads) and `db.sqlite` (binary) are the usual ones to exclude.

## Maintenance

- **Reset** — delete `.astria/` and re-run `astria run .`. Nothing outside the directory is mutated (the global store and embed cache are separate).
- **Stale?** — `astria status` reports graph freshness and last-build time; `astria update` rebuilds incrementally from the file manifest.
- **Suspect the data?** — `astria diagnose` is a read-only health check (dangling edges, duplicates, stubs).
- **Pre-1.0 layout?** — `astria migrate` renames `.graphify/` → `.astria/`, `.graphifyignore` → `.astriaignore`, and moves the old global store.
