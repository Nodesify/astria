---
sidebar_position: 3
title: Environment variables
description: Every environment variable nodesify-graphify reads — LLM backends, embeddings, query logging, and hook-guard.
keywords: [environment variables, GRAPHIFY_LLM, GRAPHIFY_EMBED_CACHE_DIR, GRAPHIFY_QUERY_LOG, configuration]
---

# Environment variables

Every variable is optional — the default pipeline is fully local and needs none of them.

## LLM semantic enrichment

Activates the `enrich_with_semantics()` pipeline stage (docs, papers, images → concept nodes). See [Semantic enrichment](../guides/semantic-enrichment) for behavior.

| Variable | Purpose |
|---|---|
| `GRAPHIFY_LLM_BACKEND` | Selects the backend explicitly: `claude`, `openai`, or `gemini` |
| `GRAPHIFY_LLM_API_KEY` | API key — Anthropic Claude by default, or the OpenAI-compatible provider when `GRAPHIFY_LLM_BASE_URL` is set |
| `GRAPHIFY_LLM_BASE_URL` | Endpoint for any OpenAI-compatible provider (OpenAI, DeepSeek, Ollama, LM Studio, custom) |
| `GRAPHIFY_LLM_MODEL` | Overrides the default model for the selected backend |
| `GRAPHIFY_LLM_CONCURRENCY` | Size of the parallel LLM worker pool |
| `GRAPHIFY_LLM_PROVIDER` | Legacy alias: if set together with `GRAPHIFY_LLM_BASE_URL`, forces the OpenAI-compatible backend |
| `OPENAI_API_KEY` | Fallback key for the OpenAI-compatible backend |
| `GEMINI_API_KEY` / `GOOGLE_API_KEY` | Key for the Gemini backend |

Without a backend configured, builds stay fully offline and deterministic. Per-run overrides without env vars: `nodesify-graphify run . --backend openai --model gpt-4o-mini`.

Keys are sent in request headers (the Gemini key never goes in the URL, where it would leak into logs and history). When `GRAPHIFY_LLM_BASE_URL` points at a plain-`http` endpoint that is not local (localhost, `127.0.0.1`, `[::1]` — Ollama/LM Studio setups are silent), a warning is printed because the API key travels unencrypted.

## Local embeddings

| Variable | Purpose |
|---|---|
| `GRAPHIFY_EMBED_CACHE_DIR` | Overrides where the embedding model is cached (default `~/.graphify-embed-cache`; ~90 MB downloaded once, then offline) |

## Query logging

Appends a JSONL line (ts, kind, question, nodes, duration) per query for agent/tooling consumption. Logging never breaks a query — it fails silent.

| Variable | Purpose |
|---|---|
| `GRAPHIFY_QUERY_LOG` | Path of the JSONL log file; `GRAPHIFY_QUERY_LOG=1` uses the default location |
| `GRAPHIFY_QUERY_LOG_ENABLE` | `1` turns logging on without choosing a path |
| `GRAPHIFY_QUERY_LOG_DISABLE` | `1` always wins — logging is off regardless of the other two |

## Hook guard

The editor `PreToolUse` guard (see [Agent integration](../guides/mcp-and-agents#editor-guard-hook-guard)). These are read by the hook process, so set them in your editor/agent environment, not your shell profile.

| Variable | Purpose |
|---|---|
| `GRAPHIFY_HOOK_STRICT` | `1` enables strict mode (gates un-indexed reads); `0` or the `--strict` flag also work |
| `GRAPHIFY_HOOK_STRICT_TTL` | Seconds a graph stays considered fresh in strict mode (default `1800`) |
