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
| `ASTRIA_LLM_BACKEND` | Required opt-in selection: `claude` (or `anthropic`), `openai` (or `openai-compatible`/`openai_compatible`), `azure` (or `azure-openai`), `bedrock` (or `aws`), `kimi` (or `moonshot`), or `gemini` (or `google`); `none` disables enrichment |
| `ASTRIA_LLM_API_KEY` | API key for the explicitly selected backend; does not select or activate a backend |
| `ASTRIA_LLM_BASE_URL` | Endpoint for any OpenAI-compatible provider (OpenAI, DeepSeek, Ollama, LM Studio, custom) |
| `ASTRIA_LLM_MODEL` | Overrides the default model for the selected backend (for Bedrock: the full model id, e.g. `anthropic.claude-3-5-sonnet-20241022-v2:0`) |
| `ASTRIA_LLM_CONCURRENCY` | Size of the parallel LLM worker pool (default `4`, clamped to 1–8) |
| `ASTRIA_LLM_BUDGET` | Total LLM token budget (input + output) per run; `0` or unset = unlimited. When the budget is exhausted, extraction stops loudly before publishing; engine calls, judge calls, and usage-less responses all count toward it |
| `ASTRIA_LLM_COMMUNITY_MAX` | Cap on community-naming LLM calls per run for `--label-communities` (default `48`) |
| `OPENAI_API_KEY` | Fallback key for the OpenAI-compatible backend |
| `OPENAI_BASE_URL` | Fallback base URL for the OpenAI-compatible backend when `ASTRIA_LLM_BASE_URL` is unset |
| `GEMINI_API_KEY` / `GOOGLE_API_KEY` | Key for the Gemini backend |

## Azure OpenAI backend

`ASTRIA_LLM_BACKEND=azure` authenticates with Azure's `api-key` header against a resource deployment (the OpenAI-compatible backend cannot produce this auth shape). Requests go to `{endpoint}/openai/deployments/{deployment}/chat/completions?api-version={version}`.

| Variable | Purpose |
|---|---|
| `ASTRIA_AZURE_ENDPOINT` | Resource endpoint, e.g. `https://my-resource.openai.azure.com` (fallback `AZURE_OPENAI_ENDPOINT`) |
| `ASTRIA_AZURE_DEPLOYMENT` | Deployment name hosting the chat model — the deployment, not the model name (fallback `AZURE_OPENAI_DEPLOYMENT_NAME`) |
| `ASTRIA_AZURE_API_KEY` | Resource API key (fallback `AZURE_OPENAI_API_KEY`) |
| `ASTRIA_AZURE_API_VERSION` | API version query parameter (default `2024-10-21`) |

## AWS Bedrock backend

`ASTRIA_LLM_BACKEND=bedrock` calls the Bedrock Converse API with real SigV4 request signing, so standard AWS credentials (static keys or `AWS_SESSION_TOKEN` temporary credentials) work directly — no bearer-token proxy required. Requires `ASTRIA_LLM_MODEL` to name a Bedrock model id.

| Variable | Purpose |
|---|---|
| `ASTRIA_AWS_REGION` | AWS region for the Bedrock runtime endpoint (fallbacks: `AWS_REGION`, `AWS_DEFAULT_REGION`) |
| `ASTRIA_AWS_ACCESS_KEY_ID` | IAM access key (fallback `AWS_ACCESS_KEY_ID`) |
| `ASTRIA_AWS_SECRET_ACCESS_KEY` | IAM secret key (fallback `AWS_SECRET_ACCESS_KEY`) |
| `ASTRIA_AWS_SESSION_TOKEN` | Optional session token for temporary credentials (fallback `AWS_SESSION_TOKEN`) |

## Kimi (Moonshot) backend

`ASTRIA_LLM_BACKEND=kimi` is the OpenAI-compatible surface pointed at Moonshot's endpoint. Default model `kimi-k2-0905-preview`.

| Variable | Purpose |
|---|---|
| `ASTRIA_KIMI_BASE_URL` | Overrides the endpoint (default `https://api.moonshot.cn/v1`) |
| `MOONSHOT_API_KEY` / `KIMI_API_KEY` | Kimi API key (also honors `ASTRIA_LLM_API_KEY`, then `OPENAI_API_KEY`) |

## Cost reporting

Every `run`/`update` writes `.astria/cost.json` with the run's measured token spend plus lifetime totals. These two optional rates add a dollar estimate:

| Variable | Purpose |
|---|---|
| `ASTRIA_COST_INPUT_PER_MTOK` | USD per 1M input tokens; both rates must be set for an estimate to appear |
| `ASTRIA_COST_OUTPUT_PER_MTOK` | USD per 1M output tokens |

## MCP HTTP serving

| Variable | Purpose |
|---|---|
| `ASTRIA_MCP_TOKEN` | Default bearer token for `astria mcp --http` (the `--token` flag overrides). Required whenever the server binds a non-loopback host |

Keys and endpoint variables do not activate enrichment. Without explicit backend selection, the pipeline makes no LLM calls. Per-run overrides without env vars: `astria run . --backend openai --model gpt-4o-mini`.

Keys are sent in request headers (the Gemini key never goes in the URL, where it would leak into logs and history). When `ASTRIA_LLM_BASE_URL` points at a plain-`http` endpoint that is not local (localhost, `127.0.0.1`, `[::1]` — Ollama/LM Studio setups are silent) **and an API key is set**, a warning is printed because the API key travels unencrypted.

## Jev judge layer

Optional decision layer over the selected backend (`--judge jev` / `ASTRIA_LLM_JUDGE=jev`). Requires an explicit backend; judge calls count toward `ASTRIA_LLM_BUDGET` and fingerprint into the extraction cache. See [Semantic enrichment](../guides/semantic-enrichment#jev-judge-layer).

| Variable | Purpose |
|---|---|
| `ASTRIA_LLM_JUDGE` | Judge selection: `jev` (TypeSafe System One). Requires `ASTRIA_LLM_BACKEND`; the judge wraps an engine and cannot generate extractions on its own |
| `ASTRIA_LLM_JUDGE_API_KEY` | Judge API key (falls back to `TYPESAFE_API_KEY`) |
| `ASTRIA_LLM_JUDGE_MODEL` | Judge model (default `jev-latest`) |
| `ASTRIA_LLM_JEV_VERIFY` | `off` disables the per-file verification pass (relation/node-type re-choice + edge existence) |
| `ASTRIA_LLM_JEV_MIN_EDGE_PROBABILITY` | Edges whose existence probability is below this are dropped (default `0.40`) |
| `ASTRIA_LLM_JEV_GATE` | `off` disables the batched trivial-file gate |
| `ASTRIA_LLM_JEV_GATE_CACHE` | `off` re-judges every run. Default on: gate verdicts are cached per (file content hash, judge configuration), so judge-gated runs are reproducible and unchanged files cost no judge calls on re-runs |
| `ASTRIA_LLM_JEV_GATE_MAX_BYTES` | Files larger than this skip the gate and are always enriched (default `65536`) |
| `ASTRIA_LLM_JEV_GATE_DROP_THRESHOLD` | Gate drop probability above this skips the file (default `0.40`) |
| `ASTRIA_LLM_JEV_GATE_BATCH` | Files judged per gate request (default `50`, clamped to 1–200) |
| `ASTRIA_QUERY_DEBUG_SCORES` | `1` (or `true`/`on`) dumps the top scored nodes with their match components and the final seed list to stderr — diagnose a retrieval miss from one query run |
| `ASTRIA_QUERY_MIN_SEMANTIC_CONFIDENCE` | Hard floor for SEMANTIC edges at query time (default `0.0` = off). Verified edges whose calibrated keep-probability falls below the floor are excluded from traversal; structural and inferred edges are never filtered. Lets graph consumers act on the judge's verdicts at query time |
| `ASTRIA_QUERY_SEED_FLOOR` | `off` disables the no-confident-match guard. Default on: when no node matches any of the question's key (highest-IDF) terms and no node matched even half of the effective terms, the query returns an explicit miss (naming the missing vocabulary) instead of a full budget of incidental word matches — one common word fully covering one label is real evidence, but it does not identify an answer. A qualifying embedding candidate, entry-intent questions, and docs-majority corpora always bypass the guard |
| `ASTRIA_CORPUS_MODE` | Pins the corpus mode `docs` or `code` instead of auto-detection from the prose share (default auto: ≥95% prose nodes → docs-majority, which opens the doc-seed quota and ranks chunks like documents). Unrecognized values warn and fall back to auto. The active non-default mode is disclosed in the query header |

## Local embeddings

| Variable | Purpose |
|---|---|
| `ASTRIA_EMBED_CACHE_DIR` | Overrides where the embedding model is cached (default `~/.astria-embed-cache`; ~615 MB downloaded once, then offline) |
| `ASTRIA_EMBED` | `off` (or `0`/`false`/`no`) stops queries from auto-merging embedding seeds. Build-side `--embed` still computes vectors; this only turns off consuming them, so a graph that carries vectors can still be queried structurally. The `astria query --no-embed` flag sets the same switch per call. |
| `ASTRIA_CHUNK_CHARS` | Overrides the document chunk size in characters (default `1200`, clamped to 400–8000). Larger chunks mean fewer, coarser document nodes; smaller chunks mean finer evidence granularity. Takes effect on the next fresh extraction — delete the graph directory (or change it before the first build) after changing it. |

## Media transcription

`run`/`update` transcribe video and audio files (`mp4`, `mov`, `webm`, `mkv`, `avi`, `mp3`, `wav`, `m4a`, `flac`, `ogg`, `opus`, `aac`, `wma`) through the external [`whisper-cli` binary (whisper.cpp)](https://github.com/ggml-org/whisper.cpp); video files also need [`ffmpeg`](https://ffmpeg.org) on PATH to demux the audio track, audio-only repositories transcribe without it. Transcripts enter the graph as document nodes like any markdown file. Without the binaries or a model the files are skipped with a one-line notice — the pipeline never fails because of media. See [CLI reference](./cli#building-the-graph).

| Variable | Purpose |
|---|---|
| `ASTRIA_WHISPER_MODEL` | Path to a whisper.cpp ggml `.bin` model file. Unset, astria uses the first `*.bin` in `<project>/.astria/models/`, then in `~/.astria/models/` |

## Google Workspace

`.gdoc`/`.gsheet`/`.gslides` shortcut files are resolved to Drive file ids and exported through the Drive API v3 (`files.export`). Missing credentials skip the file with a one-line notice — builds never fail on Google Workspace files.

| Variable | Meaning |
|---|---|
| `ASTRIA_GDRIVE_ACCESS_TOKEN` | OAuth bearer token with the Drive read-only scope. Unset, astria refreshes via a gcloud Application Default Credentials file (`authorized_user` JSON from `gcloud auth application-default login`), honoring `GOOGLE_APPLICATION_CREDENTIALS` when set |

## Query logging

Appends a JSONL line (ts, kind, question, nodes, duration_ms) per query for agent/tooling consumption. Logging never breaks a query — it fails silent.

| Variable | Purpose |
|---|---|
| `ASTRIA_QUERY_LOG` | Path of the JSONL log file; any non-empty value is used verbatim as the path (there is no `=1` shortcut — use `ASTRIA_QUERY_LOG_ENABLE` for the default location) |
| `ASTRIA_QUERY_LOG_ENABLE` | `1` turns logging on with the default path (`~/.cache/astria-queries.log`) |
| `ASTRIA_QUERY_LOG_DISABLE` | `1` always wins — logging is off regardless of the other two |

## Export targets

| Variable | Purpose |
|---|---|
| `NEO4J_USERNAME` | Username for `astria export --neo4j-push` (default `neo4j`; the `--neo4j-user` flag overrides) |
| `NEO4J_PASSWORD` | Password for `astria export --neo4j-push` (empty by default; the `--neo4j-pass` flag overrides) |

## Hook guard

The editor `PreToolUse` guard (see [Agent integration](../guides/mcp-and-agents#editor-guard-hook-guard)). These are read by the hook process, so set them in your editor/agent environment, not your shell profile.

| Variable | Purpose |
|---|---|
| `ASTRIA_HOOK_STRICT` | `1` enables strict mode (gates un-indexed reads); `0` disables it and overrides the `--strict` flag; the `--strict` flag alone also enables strict mode when the variable is unset |
| `ASTRIA_HOOK_STRICT_TTL` | Seconds a graph stays considered fresh in strict mode (default `1800`) |
