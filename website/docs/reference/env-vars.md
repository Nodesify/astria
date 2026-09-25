---
sidebar_position: 3
title: Environment variables
description: Every environment variable astria reads — LLM backends, embeddings, query logging, and hook-guard.
keywords: [environment variables, ASTRIA_LLM, ASTRIA_EMBED_CACHE_DIR, ASTRIA_QUERY_LOG, configuration]
---

# Environment variables

Every variable is optional — the default pipeline is fully local and needs none of them.

## LLM semantic enrichment

Activates the `enrich_with_semantics()` pipeline stage (docs, papers, images → concept nodes). See [Semantic enrichment](../guides/semantic-enrichment) for behavior.

| Variable | Purpose |
|---|---|
| `ASTRIA_LLM_BACKEND` | Selects the backend explicitly: `claude`, `openai`, or `gemini` |
| `ASTRIA_LLM_API_KEY` | API key — Anthropic Claude by default, or the OpenAI-compatible provider when `ASTRIA_LLM_BASE_URL` is set |
| `ASTRIA_LLM_BASE_URL` | Endpoint for any OpenAI-compatible provider (OpenAI, DeepSeek, Ollama, LM Studio, custom) |
| `ASTRIA_LLM_MODEL` | Overrides the default model for the selected backend |
| `ASTRIA_LLM_CONCURRENCY` | Size of the parallel LLM worker pool |
| `ASTRIA_LLM_PROVIDER` | Legacy alias: if set together with `ASTRIA_LLM_BASE_URL`, forces the OpenAI-compatible backend |
| `OPENAI_API_KEY` | Fallback key for the OpenAI-compatible backend |
| `GEMINI_API_KEY` / `GOOGLE_API_KEY` | Key for the Gemini backend |

Without a backend configured, builds stay fully offline and deterministic. Per-run overrides without env vars: `astria run . --backend openai --model gpt-4o-mini`.

Keys are sent in request headers (the Gemini key never goes in the URL, where it would leak into logs and history). When `ASTRIA_LLM_BASE_URL` points at a plain-`http` endpoint that is not local (localhost, `127.0.0.1`, `[::1]` — Ollama/LM Studio setups are silent), a warning is printed because the API key travels unencrypted.

## Local embeddings

| Variable | Purpose |
|---|---|
| `ASTRIA_EMBED_CACHE_DIR` | Overrides where the embedding model is cached (default `~/.astria-embed-cache`; ~90 MB downloaded once, then offline) |

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
