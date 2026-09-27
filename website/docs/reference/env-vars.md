---
sidebar_position: 3
title: Environment variables
description: Every environment variable astria reads — LLM backends, embeddings, query logging, and hook-guard.
keywords: [environment variables, ASTRIA_LLM, ASTRIA_EMBED_CACHE_DIR, ASTRIA_QUERY_LOG, configuration]
---

# Environment variables

Every variable is optional — the default pipeline is fully local and needs none of them.

> Legacy `GRAPHIFY_*` configuration names remain fallback inputs where supported. Backend activation is an exception: only `ASTRIA_LLM_BACKEND` or the CLI `--backend` flag opts into LLM enrichment; `GRAPHIFY_LLM_BACKEND` does not activate it.

## LLM semantic enrichment

Activates the `enrich_with_semantics()` pipeline stage (docs, papers, images → concept nodes). See [Semantic enrichment](../guides/semantic-enrichment) for behavior.

| Variable | Purpose |
|---|---|
| `ASTRIA_LLM_BACKEND` | Required opt-in selection: `claude`, `openai`, or `gemini`; `none` disables enrichment |
| `ASTRIA_LLM_API_KEY` | API key for the explicitly selected backend; does not select or activate a backend |
| `ASTRIA_LLM_BASE_URL` | Endpoint for any OpenAI-compatible provider (OpenAI, DeepSeek, Ollama, LM Studio, custom) |
| `ASTRIA_LLM_MODEL` | Overrides the default model for the selected backend |
| `ASTRIA_LLM_CONCURRENCY` | Size of the parallel LLM worker pool |
| `OPENAI_API_KEY` | Fallback key for the OpenAI-compatible backend |
| `OPENAI_BASE_URL` | Fallback base URL for the OpenAI-compatible backend when `ASTRIA_LLM_BASE_URL` is unset |
| `GEMINI_API_KEY` / `GOOGLE_API_KEY` | Key for the Gemini backend |

Keys and endpoint variables do not activate enrichment. Without explicit backend selection, the pipeline makes no LLM calls. Per-run overrides without env vars: `astria run . --backend openai --model gpt-4o-mini`.

Keys are sent in request headers (the Gemini key never goes in the URL, where it would leak into logs and history). When `ASTRIA_LLM_BASE_URL` points at a plain-`http` endpoint that is not local (localhost, `127.0.0.1`, `[::1]` — Ollama/LM Studio setups are silent), a warning is printed because the API key travels unencrypted.

## Local embeddings

| Variable | Purpose |
|---|---|
| `ASTRIA_EMBED_CACHE_DIR` | Overrides where the embedding model is cached (default `~/.astria-embed-cache`; ~90 MB downloaded once, then offline) |
| `ASTRIA_EMBED` | `off` (or `0`/`false`/`no`) stops queries from auto-merging embedding seeds. Build-side `--embed` still computes vectors; this only turns off consuming them, so a graph that carries vectors can still be queried structurally. The `astria query --no-embed` flag sets the same switch per call. |
| `ASTRIA_CHUNK_CHARS` | Overrides the document chunk size in characters (default `1200`, clamped to 400–8000). Larger chunks mean fewer, coarser document nodes; smaller chunks mean finer evidence granularity. Takes effect on the next fresh extraction — delete the graph directory (or change it before the first build) after changing it. |

## Query logging

Appends a JSONL line (ts, kind, question, nodes, duration) per query for agent/tooling consumption. Logging never breaks a query — it fails silent.

| Variable | Purpose |
|---|---|
| `ASTRIA_QUERY_LOG` | Path of the JSONL log file; `ASTRIA_QUERY_LOG=1` uses the default location |
| `ASTRIA_QUERY_LOG_ENABLE` | `1` turns logging on without choosing a path |
| `ASTRIA_QUERY_LOG_DISABLE` | `1` always wins — logging is off regardless of the other two |

## Hook guard

The editor `PreToolUse` guard (see [Agent integration](../guides/mcp-and-agents#editor-guard-hook-guard)). These are read by the hook process, so set them in your editor/agent environment, not your shell profile.

| Variable | Purpose |
|---|---|
| `ASTRIA_HOOK_STRICT` | `1` enables strict mode (gates un-indexed reads); `0` or the `--strict` flag also work |
| `ASTRIA_HOOK_STRICT_TTL` | Seconds a graph stays considered fresh in strict mode (default `1800`) |
