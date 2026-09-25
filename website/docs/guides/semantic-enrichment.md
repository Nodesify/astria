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
