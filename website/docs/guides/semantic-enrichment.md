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

Select an LLM backend explicitly with `--backend` or `ASTRIA_LLM_BACKEND` to enrich documents, papers, and images. Credentials or endpoint variables alone never activate network enrichment. `--backend none` or `ASTRIA_LLM_BACKEND=none` disables it.

| Backend | Env vars | Vision |
|---------|----------|--------|
| Anthropic Claude (`claude`) | `ASTRIA_LLM_API_KEY` | ✓ |
| OpenAI-compatible (`openai`: OpenAI, DeepSeek, Ollama, LM Studio, custom) | `ASTRIA_LLM_BASE_URL` + `ASTRIA_LLM_API_KEY`/`OPENAI_API_KEY` | ✓ |
| Google Gemini (`gemini`) | `GEMINI_API_KEY` or `GOOGLE_API_KEY` | ✓ |

- `ASTRIA_LLM_BACKEND` selects the backend explicitly
- `ASTRIA_LLM_MODEL` overrides the model
- Per-run: `astria run . --backend openai --model gpt-4o-mini`
- Images (png/jpg/webp/gif, ≤5 MB) go through each backend's vision API
- `ASTRIA_LLM_CONCURRENCY` controls the parallel worker pool; long files are chunked and LLM output is validated

Once selected, a backend reads its configured credentials, including the generic provider environment variables listed above. Without explicit selection, the structural pipeline does not invoke an LLM. `--label-communities` and `--deep` also require explicit backend selection.

### Jev judge layer — `--judge jev`

`astria run . --backend openai --judge jev` keeps the engine backend as the generator and layers TypeSafe's Jev on top of it. Jev is a System One decision model — typed judgments with calibrated probabilities, not a text generator — so it never writes the extraction itself; it re-judges what the engine produced:

- **Gates trivial files** before their first extraction with batched keep/drop judgments (≈1 request per 50 files), so empty or trivial files never cost an engine call. Gated files keep their structural extraction; the run summary reports the count ("N files gated by Jev").
- **Re-judges every extraction** in one request per file: relations and node types are re-chosen from the schema allowlists (replacing the lossy clamps), and every edge gets a keep/drop existence verdict. Spurious edges are dropped; kept edges carry the judge's keep probability as a calibrated `confidence_score` in the graph.
- **Re-ranks the suggested questions** in `graph_report.md` so the most useful one leads (only on runs that rebuilt the graph).

Judge decisions are cheap and batched, count toward `ASTRIA_LLM_BUDGET` like every other call, and fingerprint into the extraction cache — changing the judge or its configuration invalidates cached extractions. The judge requires an explicit backend (it wraps an engine; it cannot generate extractions — `--backend jev` is rejected with a pointer to `--judge`).

### Cost: measured, capped, and cached

Every backend response's usage block is counted across the whole run — extraction, community naming, and deep linking — and the run summary prints it:

```
LLM usage: 17 API calls, 3366 in / 1675 out tokens
```

- `ASTRIA_LLM_BUDGET` (total tokens) stops extraction before the cap is exceeded; remaining files fail loudly instead of silently skipping.
- `pipeline_runs` records each run's `llm_input_tokens` / `llm_output_tokens` / `llm_api_calls`, so spend is queryable history, not a vibe.
- Extraction caches include file content plus the effective backend, endpoint, model, and prompt configuration. Matching inputs reuse output; configuration changes invalidate it. Cached and fresh output follow the same merge path.
- Failed semantic extraction leaves the core graph and successful-file manifest unadvanced, so a later update retries the work. Derived community-label and deep-link stages run after the core commit and can be retried separately on the next run.

### Thematic community labels — `--label-communities`

`astria run . --backend openai --label-communities` names communities with **one LLM call per changed community**:

```
Communities labeled: 6 (--label-communities)
...
Communities labeled: 0, 6 unchanged (--label-communities)   # second run: all reused
```

- The label ships with a one-line **summary**, and both are first-class data: MCP `list_communities`, `graph_report.md`, `graph.json` (`communities` array), wiki, and Obsidian all show them.
- Provenance is explicit — `label_source` is `llm` or `hub`, so a themed label is always distinguishable from the deterministic thematic/hub term that names the community otherwise.
- Caching includes community membership, prompt inputs, and effective backend configuration: changed members, prompts, endpoint, or model require fresh output. `ASTRIA_LLM_COMMUNITY_MAX` caps calls per run (default 48, largest communities first); communities under 3 nodes keep deterministic names.

### Deep concept links — `--deep`

`--deep` is the second extraction tier: one LLM call per file offers the file's code symbols against the graph's concept nodes and writes the meaningful matches as `INFERRED` edges tagged `context='deep'` — the cross-file concept mesh the AST cannot see.

- Results are cached against file content, offered symbols and concepts, prompts, and effective backend configuration. Changes to any of those inputs can require a fresh call.
- Source changes invalidate deep edges. Run with `--deep` again to replay valid cached links or regenerate links for changed inputs; ordinary updates do not restore them. `--detail high` continues to filter deep links as inferred evidence.
```
Deep concept links: 27 (--deep)
```
