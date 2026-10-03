---
sidebar_position: 1
title: CLI reference
description: Every astria command and flag — building, querying, exporting, memory, the cross-repo global graph, and assistant integration.
keywords: [cli, commands, flags, reference]
---

# CLI reference

All query and read commands accept `--graph .` to point at an existing `.astria/` directory (defaults to the current directory); mutating commands (`run`, `update`, `watch`, `merge`, `diff`, `global`, `add`, `save-result`, `reflect`, `install`) operate on their own arguments instead — `add`, `save-result`, and `reflect` also accept `--graph`.

## Building the graph

```bash
astria run <path>                 # Full pipeline: detect → extract → build → cluster → analyze → report
astria run <path> --wiki          # ...also export a markdown wiki to .astria/wiki
astria run <path> --embed         # ...also compute local embeddings (similar_to edges + semantic query recall)
astria run <path> --backend openai --model gpt-4o-mini  # ...with LLM semantic enrichment (see Semantic enrichment)
astria run <path> --global --as <tag>  # ...also merge this repo into the cross-repo global graph (see Global graph)
astria update <path>              # Reuse cached ASTs, reconcile current corpus; regenerate an existing wiki
astria watch <path> [--debounce 3000]  # Watch for file changes, auto-rebuild
astria cluster-only <path> [--resolution 0.5] [--exclude-hubs]  # Re-cluster + analyze + report without re-extracting
astria merge <pathA> <pathB> <outPath> [--same-repo]  # Merge two graphs (namespaced per repo; --same-repo shares ids and errors on conflicts)
astria diff <pathA> <pathB>       # Compare two graphs
astria merge-driver install       # Union-merge .astria/graph.json on parallel-branch commits (see Team serving & CI)
```

`cluster-only` accepts two clustering controls: `--resolution <r>` (0.0–1.0, default 0.0 = classic label propagation) requires a minimum share of a node's neighbors to agree on the winning community before the node joins it — higher values produce more, smaller communities — and `--exclude-hubs` keeps high-degree hub nodes out of propagation so they cannot glue every community into one (hubs are attached to their strongest community afterwards, so every node still lands in one).

`run` and `update` accept `--backend <claude|openai|gemini|none>`, `--model <name>`, and `--judge <name>` (per-run LLM enrichment without env vars; `--judge jev` layers the TypeSafe judge over the backend), `--no-dedup` (skip near-duplicate semantic entity merging), `--label-communities` (LLM thematic names for changed communities), and `--deep` (per-file concept linking) — the two LLM features require `--backend` and are described in [Semantic enrichment](../guides/semantic-enrichment). `--judge` requires `--backend`. `update` additionally accepts `--embed` (compute/recompute local embeddings, same as `run --embed`), `--quiet` (suppress progress lines and the token benchmark, used by git hooks), and `--if-stale <minutes>` (skip when the graph was published less than N minutes ago). Code definitions retain their identities even when labels match across classes or files.

Backend selection must be explicit through `--backend` or `ASTRIA_LLM_BACKEND`; credentials alone do not activate it. `none` disables enrichment. Cached ASTs avoid reparsing unchanged files, while references are reconciled across the current corpus. Graph facts and the manifest commit together after successful extraction; derived passes rerun after that commit and can be retried on the next update.

Builds also pick up, automatically:

- **Cargo workspaces** — when a `Cargo.toml` is present, workspace members and internal path dependencies become `crate::*` nodes with `crate_depends_on` edges (honoring `package =` renames and `workspace = true` inheritance). No LLM involved — dependency structure is fact.
- **MCP configs** — `.mcp.json`, `mcp_servers.json`, and `claude_desktop_config.json` become `mcp_server`/`mcp_command`/`mcp_package` nodes with `requires_env` edges (env **names only** — values are never read).
- **Video/audio transcription** — `mp4`/`mov`/`webm`/`mkv`/`avi` video and `mp3`/`wav`/`m4a`/`flac`/`ogg`/`opus`/`aac`/`wma` audio files are transcribed during `run`/`update` when [whisper.cpp](https://github.com/ggml-org/whisper.cpp)'s `whisper-cli` is on PATH (video also needs `ffmpeg`); the transcript becomes document and chunk nodes exactly like a markdown file. The model comes from `ASTRIA_WHISPER_MODEL` or the first `*.bin` in `<project>/.astria/models/` or `~/.astria/models/`. Missing binaries or model skip the files with a one-line notice — builds never fail on media.
- **Media URLs** — `astria add <url>` accepts YouTube/Youtube-music, Vimeo, Dailymotion and Twitch links (plus direct `.mp4`/`.mp3`/… URLs): the audio track is downloaded with [yt-dlp](https://github.com/yt-dlp/yt-dlp) (16 kHz mono WAV when `ffmpeg` is available) and transcribed by the same whisper pipeline as local media. The download is vetted before any media byte moves: yt-dlp first resolves the destination in metadata-only mode and every post-redirect media URL must pass the same private/internal-network policy as the regular fetch path; a download that would land inside your network is refused. yt-dlp missing degrades to a stub node with the install hint.
- **Office documents** — `.docx` (headings, paragraphs, lists, tables) and `.xlsx` (one markdown table per sheet, capped at 500 rows / 30 columns) are extracted locally and become document nodes.
- **Google Workspace** — `.gdoc`/`.gsheet`/`.gslides` Drive shortcut files resolve to a file id and export as text/CSV → markdown through the Drive API; see [Google Workspace env vars](./env-vars.md#google-workspace). Missing credentials skip the file with a one-line notice. Cloud edits never change the local shortcut bytes, so plain `run`/`update` probe the remote revision for unchanged shortcuts and rebuild when it has moved — remote edits are picked up without touching the file. Offline (or without credentials) the cached extraction stands: remote freshness requires connectivity.
- **More doc formats** — `.qmd` rides the markdown path; `.html`/`.htm` are tag-stripped (scripts/styles dropped); `.yaml`/`.yml` are chunked as text.
- **More languages** — 42 registered languages: SQL (tables, views, functions/procedures, triggers incl. `CREATE TRIGGER`), Julia, R, Fortran, Solidity, Groovy, Luau, Objective-C, OCaml (`.ml`) + OCaml Interface (`.mli`), Common Lisp, BYOND DreamMaker, Astro; Vue and Svelte extract their embedded `<script>` TS/JS through the JavaScript/TypeScript grammars; VB.NET and Pascal/Delphi use regex-declaration extraction (no grammar crate exists upstream).
- **Transcript sidecars** — any `.txt`/`.md` you drop into `.astria/transcripts/` is ingested as document nodes on the next run/update (handy for tools or languages the built-in transcription does not cover). The contract for external transcribers: run any tool you like, write the text there, let the graph index it — or pipe it through the built-in writer: `astria add --transcript <file>` keeps the file's name, `astria add --transcript -` reads piped stdin (e.g. `whisper ... | astria add --transcript -`) and stores it under a timestamped name.

## Querying

```bash
astria explain <node> [--graph .] [--json]      # Explain a node and its connections
astria query <question> [--dfs] [--depth 2] [--budget 2000] [--directed] [--detail high] [--cursor N] [--no-embed] [--json] [--graph .]  # BFS/DFS traversal
astria path <A> <B> [--directed] [--detail high] [--json] [--graph .]   # Shortest path between two concepts
astria affected <node> [--depth 2] [--relation R] [--json] [--graph .]  # Blast radius - what breaks if you change this node
astria map [--budget 2000] [--detail high] [--json] [--graph .]  # PageRank-ranked repo map with top symbols
astria stats [--graph .] [--json]               # Node/edge/community counts
astria god-nodes [--graph .] [--json]           # Highest-degree hub nodes (MCP god_nodes parity)
astria communities [--graph .] [--json]         # Communities with labels, size, cohesion (MCP list_communities parity)
astria neighbors <node> [--relation R] [--json] [--graph .]  # A node's neighbors, optionally one relation (MCP get_neighbors parity)
astria status [--graph .] [--json]              # Graph freshness, staleness, and build provenance
astria callflow <node> [--depth 2] [--direction out|in|both] [--out flow.md] [--graph .]  # Mermaid call graph
astria history [--limit 20] [--graph .]        # Show recent query history
```

### Query flags {#query-flags}

- `--dfs` — depth-first instead of breadth-first traversal
- `--depth N` — maximum traversal depth
- `--budget N` — maximum query text tokens using `o200k_base`, including headers and continuation metadata (default 2000)
- `--directed` — follow edge direction instead of treating the graph as undirected
- `--detail high` — fidelity tier: only `EXTRACTED`/`DECLARED` facts (a provenance-class filter, not a numeric threshold), dropping inferred, semantic and learned edges, regardless of usage-adjusted confidence scores
- `--cursor N` — continuation cursor for truncated traversals; use the returned value, which advances through node and edge records
- `--no-embed` — skip auto-merged embedding seeds even when the graph carries vectors (same switch as `ASTRIA_EMBED=off`; see [Environment variables](./env-vars))
- `--json` — machine-readable output instead of prose: `query` reports counts, the continuation cursor, build provenance, and the answer text; `explain`/`neighbors` return the strongest 20 neighbors, each with its edge direction in `outgoing` (the native layer caps the list — `explain --json` also reports `neighborCount`, the true total); `affected` returns every hit with depth, relation, and edge provenance; `stats` returns counts and type breakdown

`affected` reports the evidence tier of every hop. `RESOLVED` (the default case) marks a call expression extracted from source whose name bound to exactly one definition — trustworthy for impact analysis. `EXTRACTED` marks directly-verified facts (contains/imports). Hops reached through `INFERRED` edges — no source locus, or a name too common to bind — are marked with a legend line, so a blast radius never presents guesswork as verified.

`god-nodes`, `communities`, and `neighbors` give the CLI the same answers the [MCP tools](./mcp-tools) expose, so scripts and non-MCP agents can reach them too.

`status` judges freshness from the graph itself, not just file timestamps: every `run`/`update` stamps the npm CLI version and the extraction-rules version into the graph, and `status --json` exposes them (`astriaVersion`, `extractionHashVersion`, `currentExtractionHashVersion`, `extractionOutdated`) so tooling can detect a graph that predates the installed binary's extraction rules. Graphs built before stamping report nulls.

CLI and MCP share hybrid retrieval and load a fresh SQLite graph snapshot for each request. Query output reports when the graph was last built, so agents can judge freshness. Repeated queries promote recurring node pairs into `learned` edges — see [learning from usage](#learning-from-usage).

Query ranking favors complete identifier matches and implementing definitions. Explicit requests for tests, examples, or documentation retain those results; inline Rust test functions are classified from their test attributes and modules. JavaScript and TypeScript extraction includes functions assigned to variables, object properties, members, and CommonJS exports. Static receiver names can resolve calls, but runtime receiver types and aliases are not inferred. Python overload declarations yield to a concrete implementation in the same scope, preserving its body and documentation.

Query budgets must be positive and large enough for metadata and the next complete record; otherwise the query returns an error. Continuation cursors address the current result's node and edge records. Start a new query after rebuilding the graph, and do not reuse cursors from older versions that counted only nodes.

### Query log (for tooling)

Every query can also append a JSONL line (ts, kind, question, nodes, duration_ms) to a log file for agent/tooling consumption:

- `ASTRIA_QUERY_LOG=<path>` — log to a specific file (any non-empty value is used verbatim as the path)
- `ASTRIA_QUERY_LOG_ENABLE=1` — turn on logging with the default location (`~/.cache/astria-queries.log`)
- `ASTRIA_QUERY_LOG_DISABLE=1` — always wins; logging never breaks a query (fails silent)

## Exports and visualization

```bash
astria export [--graph .] [--out graph.json] [--format json|html|graphml|cypher|svg|falkordb] [--mode standard|large]
astria tree [--out tree.html] [--max-children 40]   # Collapsible filesystem tree of all symbols (HTML)
astria wiki [--out .astria/wiki] [--max-nodes 25] [--format markdown|obsidian] [--graph .]  # Wikipedia-style markdown wiki, or an Obsidian vault
astria prs [20] [--conflicts] [--triage] [--queue] [--json] [--graph .]  # PR dashboard: CI state, review status, worktree mapping, ranked queue, merge-order risk (requires the gh CLI)
```

Every `run`/`update` also writes two machine-readable artifacts next to the report: `.astria/graph.json` (the full graph — nodes, edges, hyperedges, communities; the file the git merge driver union-merges) and `.astria/cost.json` (the run's measured LLM spend — this run plus lifetime totals, with a dollar estimate when `ASTRIA_COST_*_PER_MTOK` are set; see [Environment variables](./env-vars)).

`export --format html` creates a self-contained interactive graph view. The default `--mode standard` accepts graphs of at most 5,000 nodes and fails with an actionable message for larger graphs; `--mode large` lifts the cap. The viewer opens as community bubbles (click to expand into member nodes), supports search over symbols and community names, 1-hop neighborhood focus with relation-labeled links and direction arrows, and a level-of-detail "All nodes" mode that stays responsive on any repo size because positions are precomputed, physics is disabled, and only what is on screen is drawn. The page is a single file that embeds the whole graph, so very large repositories produce proportionally large HTML files.

`--format cypher` writes an idempotent Neo4j import script (MERGE statements — safe to re-run):

```bash
astria export --graph . --format cypher --out astria.cypher
cypher-shell -u neo4j -p <password> -f astria.cypher
```

See [Wiki and exports](../guides/wiki-and-exports) for details.

`--format svg` creates a deterministic static diagram. `--format falkordb` writes openCypher for FalkorDB; add `--redis-push <host:port>` and optionally `--graph-name <name>` to send it through `redis-cli`. `--neo4j-push <bolt://host:port>` sends Cypher directly to Neo4j, with `--neo4j-user` and `--neo4j-pass` (or their environment variables). `wiki --format` only distinguishes `obsidian`; any other value (including the default) produces the markdown wiki.

## Graph health

```bash
astria diagnose [--graph .] [--json]
astria health [--graph .] [--json]
astria risk [--graph .] [--staged] [--json]
astria merge-gate [--max-age-hours 24] [--min-health 60] [--max-risk 70] [--staged] [--base <ref> --head <ref>] [--json]  # CI merge gate (see Team serving & CI)
astria digest [--out .astria/digest.md] [--json]  # Engineering digest brief (see Team serving & CI)
```

Read-only health report over an existing graph: dangling edge endpoints (stub vs actionable), self-loops, duplicate edges, unclassified files, and zero-cohesion communities. `--json` emits machine-readable output. Never mutates the graph.

`health` scores code-health heuristics (unreachable-symbol candidates, file cycles, hub concentration, and staleness) from 0 to 100. `risk` maps the current git diff to impacted symbols and communities; use `--staged` to inspect only staged changes, or `--base origin/main --head HEAD` to score a committed PR range (the CI shape — a clean checkout has no working-tree diff). Both support `--json`.

## Memory and reflection

The feedback loop that complements [learned edges](#learning-from-usage): learned edges are automatic, memory is curated. Full walkthrough in [Memory and learning](../guides/memory-and-learning).

```bash
astria save-result <question> --answer <text> [--answer-file <path>] \
    [--outcome useful|dead_end|corrected] [--correction <text>] [--nodes <ids>] [--graph .]
astria reflect [--graph .]
```

- `save-result` writes a Q/A memory doc (with outcome and corrections) into `.astria/memory/`, and immediately inserts it into the graph as a document node with `references` edges to the cited code — settled questions become part of the graph right away. (Rebuilding the database does not re-ingest saved memory docs, so keep `.astria/memory/` when recreating a graph from scratch.)
- `reflect` aggregates outcomes into `.astria/reflections/LESSONS.md` with outcome tallies.

## Global graph (cross-repo)

Merge many repo graphs into one queryable store at `~/.astria/global.db` — merging behavior in detail in [Global graph](../guides/global-graph):

```bash
astria run <path> --global --as <tag>   # build, then merge into the global store
astria global add <path> [--as <tag>]   # same merge, standalone (idempotent per tag)
astria global remove <tag>              # prune a repo from the global graph
astria global list                      # registered repos
astria global path <A> <B>              # shortest path across repos
```

Design notes: sourced node ids are prefixed with the repo tag (`<tag>::<id>`); external/stub symbols stay unprefixed and dedupe by label, so `serde_json::Value` means the same thing in every repo. Same-label type declarations across repos get `same_type_as` edges (name-based unification), and parked unresolved calls are resolved when exactly one cross-repo candidate exists (fail closed on ambiguity). Query against the merged store with the usual `--graph` flag pointed at the global db:

```bash
astria query "where is the shared auth type" --graph ~/.astria/global.db
```

## Knowledge ingestion

```bash
astria add <url> [--author <name>] [--contributor <name>]  # Fetch arXiv/tweet/webpage/image/PDF into ./raw + update graph
astria add --scip <index.json>                      # Ingest a simplified SCIP JSON index (rust-analyzer & co.)
astria add --postgres <dsn>                         # Introspect a live PostgreSQL schema (requires psql on PATH)
astria add --transcript <file|->                    # Save a transcript into .astria/transcripts/ + update graph
```

Both `--scip` and `--postgres` are offline/local alternatives to URL fetching: SCIP indexes bring external toolchain symbols into the graph (`scip_impl`/`scip_typed`/`scip_def`/`scip_ref` edges, deterministic ids); Postgres introspection is read-only over the pg system catalogs (`pg_class`/`pg_namespace`/`pg_constraint` — tables/views/FKs → `contains` + `references` edges, no credentials stored). The Postgres DSN is opt-in by flag — nothing calls the network by default.

URL fetching is SSRF-guarded: only `http`/`https` schemes are accepted; each host is checked by name *and* DNS-resolved, and any loopback/private/CGNAT/link-local address (IPv4 or IPv6, including mapped forms like `::ffff:127.0.0.1`) is rejected — so cloud metadata endpoints and localhost services are unreachable no matter how the URL is spelled. Redirects are followed manually (max 5 hops) and every hop is re-validated, meaning a public server cannot bounce a fetch to an internal address. Downloads are capped at 50 MB with a 30-second timeout, and saved filenames are slugified from the URL, so a hostile URL segment cannot escape the output directory.

## Assistant integration

```bash
astria mcp [--graph .]                    # MCP over stdio (the default)
astria mcp --http [--port 8620] [--host 127.0.0.1] [--token <t>] [--projects a=./b name2=./c] [--allow-origin http://localhost:5173]  # MCP over HTTP, multi-project
astria install [--platform claude] [--all]  # Skill + MCP registration for one AI platform or all (claude, codex, gemini, cursor, copilot, aider, opencode, kiro, trae, zcode, vscode, windsurf, cline, roo, amp, pi)
astria uninstall [--platform claude] [--all] [--purge]  # Remove the install for one platform or all; --purge deep-cleans everything
astria hook install|uninstall|status  # Git hook management
astria hook-guard <mode>            # Editor PreToolUse guard (search | read | gemini) — installed into .claude/settings.json
astria merge-driver install|uninstall|run  # Gitattributes merge driver for the graph file
```

### Team serving & CI {#team-serving}

**MCP over HTTP.** `astria mcp --http` serves one or many project graphs over MCP Streamable HTTP (JSON responses): `POST /mcp` speaks the same JSON-RPC as stdio, `GET /healthz` is a liveness probe. Clients pick a project with the `x-astria-project` header or a `?project=` query parameter (unlisted names 404 — no silent fallback); the `--graph` root is the default project and `--projects name=path` adds more. Requests must carry `--token` (or `ASTRIA_MCP_TOKEN`) as a bearer token whenever the server binds a non-loopback host — unauthenticated remote serving is refused at startup, never allowed. Requests carrying an `Origin` header (browsers) are refused unless their origin is listed with `--allow-origin` — native clients send no `Origin` and always pass — and loopback serving validates the `Host` header as a DNS-rebinding defense.

**Git merge driver for the graph file.** Parallel branches both rebuild and both commit `.astria/graph.json`; `astria merge-driver install` wires a three-way union-merge driver into `.gitattributes` + git config (`merge.astria.*`) so those commits merge instead of conflicting: additions from both sides survive, a side's deletion is respected, community renumbering resolves to whichever side changed (the next `update` re-clusters anyway). `astria/graph_report.md` gets git's built-in `union` driver. `uninstall` removes the wiring.

**Merge gate.** `astria merge-gate` is the CI check the hosted tier enforces on every merge: it fails (exit 1) when the graph is missing, older than `--max-age-hours`, predates the last commit, scores below `--min-health`, or the pending diff's blast radius exceeds `--max-risk`. `--json` gives the pipeline something to parse.

**Engineering digest.** `astria digest` renders a deterministic markdown brief — graph overview, health score, hub concentration, largest communities, LLM spend from `cost.json` — for stdout or `--out <file>`. Schedule it in cron/CI for a weekly cadence.

**Docker.** A Dockerfile ships in the repo root: `docker build -t astria .` produces a slim image running the same CLI — analyze a mounted workspace (`docker run --rm -v "$PWD":/workspace astria run /workspace`) or serve the HTTP MCP on the exposed port 8620. Build with `--build-arg NAPI_FEATURES=--no-default-features` for a smaller image without the local embedding runtime (`--embed` then fails loudly, everything else works).

The hosted tier (app.graphify.com) builds on these same primitives — hosted MCP, merge-gate verification, graph-aware review, and the engineering digest — for teams that do not want to run them.

Supported platforms for `install`: `claude`, `codex`, `gemini`, `cursor`, `copilot`, `aider`, `opencode`, `kiro`, `trae`, `zcode`, `vscode`, `windsurf`, `cline`, `roo`, `amp`, `pi`. Setup walkthrough in [Agent integration](../guides/mcp-and-agents); the ten MCP tools are documented in the [MCP tools reference](./mcp-tools).

`install` also injects an always-on `## astria` instruction block into `AGENTS.md`/`CLAUDE.md` (query before grep, run `update` after edits) — idempotent, removed by `uninstall`. `hook-guard` is the editor-side companion to git hooks: it nudges agents toward `query` before raw searches and can (strict mode, opt-in) gate un-indexed reads. It fails open — any error means the tool call proceeds untouched.

## Learning from usage {#learning-from-usage}

The graph compounds in value as you query it. Every query records which (seed, discovered) node pairs its traversal connected; when the same pair recurs across **at least 2 distinct questions with 3+ total hits**, the next `run`/`update` promotes it to a `learned` edge (`INFERRED`, hits-scored, provenance `query_history`). Learned edges flow into clustering, analysis, and every export — the graph remembers which connections you actually keep asking about. High-fidelity traversals (`--detail high`) can filter them like any `INFERRED` fact.
