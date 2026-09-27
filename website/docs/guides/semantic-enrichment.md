---
sidebar_position: 2
title: Semantic enrichment
description: Two optional semantic layers — local embeddings (no API key) and LLM enrichment — that add similar_to edges and concept nodes to the graph.
keywords: [embeddings, llm, semantic, claude, openai, gemini, similar_to]
---

# Semantic enrichment

Two independent semantic layers, both optional. Without them, the graph is purely structural (AST-extracted) — still fully queryable.

## Local embeddings (no API key)

```bash
astria run . --embed
```

Downloads a small local model once (~90 MB, then offline forever) and computes vector embeddings for every node. This adds:

- `similar_to` edges (`INFERRED`, cosine-scored) linking semantically related symbols across files — they flow into clustering, surprising connections, and every export
- embedding-backed query recall: `query` merges semantic candidates with token matching, so conceptual questions with zero string overlap still find their symbols

Once embeddings exist, every `run`/`update` refreshes them incrementally (offline — the refresh never downloads), and `query` picks them up automatically.

Override the model cache location with `ASTRIA_EMBED_CACHE_DIR` — all variables in [Environment variables](../reference/env-vars).

## LLM enrichment

Set any LLM backend and the pipeline enriches docs, papers, and images into concept nodes automatically.

| Backend | Env vars | Vision |
|---------|----------|--------|
| Anthropic Claude (default) | `ASTRIA_LLM_API_KEY` | ✓ |
| OpenAI-compatible (OpenAI, DeepSeek, Ollama, LM Studio, custom) | `ASTRIA_LLM_BASE_URL` + `ASTRIA_LLM_API_KEY`/`OPENAI_API_KEY` | ✓ |
| Google Gemini | `GEMINI_API_KEY` or `GOOGLE_API_KEY` | ✓ |

- `ASTRIA_LLM_BACKEND` selects the backend explicitly
- `ASTRIA_LLM_MODEL` overrides the model
- Per-run: `astria run . --backend openai --model gpt-4o-mini`
- Images (png/jpg/webp/gif, ≤5 MB) go through each backend's vision API
- `ASTRIA_LLM_CONCURRENCY` controls the parallel worker pool; long files are chunked and LLM output is validated

Semantic enrichment is a pipeline stage — `enrich_with_semantics()` — that activates only when a backend is configured, so builds stay fully offline and deterministic without one.

### Cost: measured, capped, and cached

Every backend response's usage block is counted across the whole run — extraction, community naming, and deep linking — and the run summary prints it:

```
LLM usage: 17 API calls, 3366 in / 1675 out tokens
```

- `ASTRIA_LLM_BUDGET` (total tokens) stops extraction before the cap is exceeded; remaining files fail loudly instead of silently skipping.
- `pipeline_runs` records each run's `llm_input_tokens` / `llm_output_tokens` / `llm_api_calls`, so spend is queryable history, not a vibe.
- Extractions are cached per file content hash — an unchanged file is never re-billed on the next run.

### Thematic community labels — `--label-communities`

`astria run . --label-communities` names communities with **one LLM call per changed community**:

```
Communities labeled: 6 (--label-communities)
...
Communities labeled: 0, 6 unchanged (--label-communities)   # second run: all reused
```

- The label ships with a one-line **summary**, and both are first-class data: MCP `list_communities`, `graph_report.md`, `graph.json` (`communities` array), wiki, and Obsidian all show them.
- Provenance is explicit — `label_source` is `llm` or `hub`, so a themed label is always distinguishable from the deterministic thematic/hub term that names the community otherwise.
- Caching is a SHA-256 fingerprint of the community's member ids (`member_hash`): rebuilds preserve LLM labels while membership is unchanged and drop them the moment it changes. `ASTRIA_LLM_COMMUNITY_MAX` caps calls per run (default 48, largest communities first); communities under 3 nodes keep deterministic names.

### Deep concept links — `--deep`

`--deep` is the second extraction tier: one LLM call per file offers the file's code symbols against the graph's concept nodes and writes the meaningful matches as `INFERRED` edges tagged `context='deep'` — the cross-file concept mesh the AST cannot see.

- Results are cached per file content hash, so incremental updates only bill changed files.
- Stale links are replaced on rebuild (idempotent), and `--detail high` queries keep filtering them out like every other inferred fact.
```
Deep concept links: 27 (--deep)
```
