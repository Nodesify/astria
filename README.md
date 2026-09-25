<div align="center">

# astria

**Knowledge graph builder for codebases**

[![CI](https://github.com/Nodesify/astria/actions/workflows/ci.yml/badge.svg)](https://github.com/Nodesify/astria/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/@nodesify/astria)](https://www.npmjs.com/package/@nodesify/astria)
[![npm downloads](https://img.shields.io/npm/dm/@nodesify/astria)](https://www.npmjs.com/package/@nodesify/astria)
[![docs](https://img.shields.io/badge/docs-latest-blue)](https://nodesify.github.io/astria/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Node](https://img.shields.io/badge/node-22-339933?logo=nodedotjs&logoColor=white)](https://nodejs.org/)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/Nodesify/astria)

[Docs](https://nodesify.github.io/astria/) | [CLI Reference](https://nodesify.github.io/astria/docs/cli/) | [Architecture](ARCHITECTURE.md) | [Worked examples](worked/)

</div>

Understand a codebase before you touch it. `astria` turns any folder into a queryable knowledge graph — deterministic AST extraction in Rust, optional local-embedding semantics, zero API keys, everything on your machine.

astria is inspired by the Python [Graphify](https://github.com/safishamsi/graphify) project's core idea — turn a corpus into a queryable knowledge graph — but it is an independent, from-scratch implementation: a deterministic, offline-first Rust/tree-sitter pipeline, not a fork or a port.

You drop into an unfamiliar repo and need to know: what is load-bearing here, what breaks if I change this, where does auth live, how do these two modules connect. Reading everything costs the whole context window. The graph answers in ~3,000 tokens — **measured** at **50–110× fewer tokens per query** on real repos (printed honestly after every run, computed from real file sizes vs actual query output — [methodology and head-to-head](worked/head-to-head/)).

Three things a folder full of files can't give you:

1. **Structure that survives the session** — hub files, god nodes, communities, and the blast radius of any change, stored in SQLite and refreshed incrementally as code changes.
2. **An honest audit trail** — every edge is labeled EXTRACTED / INFERRED / AMBIGUOUS with a numeric confidence score. You always know what was found in the source versus deduced, and `--detail high` filters to only declared facts.
3. **Answers for agents and humans** — query it from the CLI, from any AI agent via MCP, or just read the exported markdown wiki with plain file links.

[Worked examples with honest reviews](worked/) — the tool run on itself, including what the graph got *wrong* — plus a [head-to-head benchmark](worked/head-to-head/) against the Python Graphify project that inspired it, run on the same corpus.

## Install

```bash
npm install -g @nodesify/astria
```

Requires no Rust toolchain — ships prebuilt native binaries via napi-rs.

> **Migrating from `@nodesify/graphify`?** 1.0 is a rebrand: the binary is `astria`, the npm package is `@nodesify/astria`, and graphs live in `.astria/` instead of `.graphify/`. Run once after installing:
>
> ```bash
> astria migrate          # renames .graphify/ -> .astria/ and the global store
> astria install          # refreshes AI-tool skills/hooks (also cleans the old graphify entries)
> ```
>
> `GRAPHIFY_*` environment variables keep working; `ASTRIA_*` takes precedence.

## What's new (unreleased)

- **Hypergraph, deterministically** — n-ary `hyperedges` (schema v7/v8) produced without an LLM: community `participate_in` groups and `shares_reference` literal groups; consumed by graph.json, report, wiki, HTML hulls, and `explain`. Graphify's hyperedges are LLM-produced; ours are local and reproducible.
- **Cross-repo global graph** — `~/.astria/global.db`: `global add/remove/list/path`, repo-tag prefixed merging that unifies external symbols across repos, `same_type_as` type edges, cross-repo call resolution (fail closed on ambiguity), `run --global --as <tag>`, and `query/explain/path --graph` against the merged store.
- **Graph health + feedback loop** — `diagnose` (read-only health report, `--json`), `save-result`/`reflect` curated memory (`.astria/memory/` → graph nodes → `LESSONS.md`) alongside automatic learned edges, build-time validation, JSONL query log (`ASTRIA_QUERY_LOG`), and always-on instruction blocks in `AGENTS.md`/`CLAUDE.md`.
- **Ingest breadth (offline-first)** — Cargo workspace + path-dep topology (auto), `.mcp.json`/`mcp_servers.json`/`claude_desktop_config.json` (env names only, never values), `add --scip <index.json>`, `add --postgres <dsn>` (read-only introspection, requires `psql`), and transcript sidecars (`.astria/transcripts/*.txt|md`).
- **SSRF-hardened URL ingestion** — `add <url>` validates every redirect hop (auto-follow is off), DNS-resolves each host and blocks loopback/private/CGNAT/link-local addresses (IPv4 and IPv6, incl. mapped forms), and slugifies downloaded filenames so a hostile URL segment cannot write outside `raw/`. The HTML export renders labels as text (no HTML interpolation), a plain-`http` LLM base URL with an API key warns, and CI audits npm dependencies alongside the existing Rust advisory check.

## What's new in 0.8.0

- **Markdown wiki export** — `wiki` / `run --wiki`: an agent-crawlable wiki (`index.md` + one article per community and god node, relative markdown links GitHub and Obsidian both navigate); `update` regenerates it so it never drifts stale
- **Obsidian vault export** — `wiki --format obsidian`: per-node notes with frontmatter tags and `[[wikilinks]]`, community overviews, and a `astria.canvas` (2,040 notes + 5,000 canvas edges on this repo)
- **Local semantic layer** — `run --embed`: a local embedding model (no API key, one-time ~90 MB download, offline after) adds `similar_to` edges across files and embedding-backed query recall; communities consolidated 401 → 194 on this repo
- **Learning from usage** — repeated queries promote recurring node pairs into `learned` edges; the graph compounds in value the more it is used
- **Neo4j export** — `export --format cypher`: idempotent MERGE script for cypher-shell
- **Token benchmark** — every run prints measured corpus-vs-query tokens (82x on this repo); worked examples with honest reviews under `worked/`
- **Security** — esbuild advisory pinned out, Windows reserved-name guards in exports, learned-edge promotion hardened against stale node references

## What's new in 0.7.0

- **Edge provenance** — every `EDGE` line in `query` output is anchored with `@file:line` and every `NODE` line with `src=file:line`; `explain` prints locations for the node and each neighbor (schema v4)
- **Reference nodes** — identifier-shaped string literals (env vars like `PLANE_URL`, snake_case keys, dotted/kebab/slash chains) become global reference nodes with `references` edges, so config/status-value usage is one graph query
- **Staleness visibility** — `query` output reports when the graph was last built, so agents can judge freshness
- **Security hardening** — no shell-string exec, literal-allowlist native module loading, install-path containment guards

## What's new in 0.6.1

- **Fixed installs shipping a stale native binary** — the platform `optionalDependencies` pins now track the package version (enforced by a test); 0.6.0 installs pulled the 0.5.0 binary

## What's new in 0.6.0

- **Safe HTML graph viewer** — `export --format html --mode standard` enforces a 5,000-node safety cap; `--mode large` opts into a precomputed-layout viewer (physics-free, key nodes first, batched search) that opens instantly on any repo size
- **Faster large-graph visualization** — Rust-computed positions, straight edges, arrow/legend caps, and a "Show all nodes" toggle replace the per-keystroke physics simulation that could hang the browser

## What's new in 0.5.0

- **Deterministic clustering** — stable communities across runs, Newman modularity in report/stats
- **Directed traversal** — `--directed` on `query`/`path` (CLI, napi, MCP), fidelity tiers (`--detail high`), continuation cursors for truncated traversals
- **Aider-style repo map** — `astria map`: PageRank-ranked files with top symbols, within a token budget
- **Node signatures** — schema v3 signatures shown in query/explain output
- **Parallel LLM semantic extraction** — `ASTRIA_LLM_CONCURRENCY` worker pool, long-file chunking, output validation, Retry-After backoff
- **Agent-facing output quality** — root-relative paths everywhere, did-you-mean suggestions, candidate lists on ambiguous seeds, node ids in query/affected output
- **Hardening** — sensitive-path denylist (.env, keys, credentials), minified/vendored asset skip, read-only commands no longer create empty `.astria/` directories
- God nodes exclude call stubs; O(V+E) blast radius via reverse adjacency; numeric confidence ranking

### 0.4.0 highlights

- `affected` blast-radius analysis, MCP server (9 tools), entity dedup (MinHash + Jaro-Winkler), `tree` HTML export, `prs` merge-order risk, dependency-manifest `pkg_*` nodes

## Usage

```bash
astria run <path>                            # Full pipeline: detect → extract → build → cluster → analyze → report
astria run <path> --wiki                     # ...also export a markdown wiki to .astria/wiki
astria run <path> --embed                    # ...also compute local embeddings (similar_to edges + semantic query recall)
astria run <path> --global --as <tag>        # ...also merge into the cross-repo global graph
astria update <path>                         # Incremental rebuild (only changed files; regenerates an existing wiki)
astria watch <path> [--debounce 3000]        # Watch for file changes, auto-rebuild
astria explain <node> [--graph .]            # Explain a node and its connections
astria query <question> [--dfs] [--depth 2] [--budget 2000] [--directed] [--detail high] [--cursor N] [--graph .]  # BFS/DFS traversal
astria path <A> <B> [--directed] [--detail high] [--graph .]  # Shortest path between two concepts
astria affected <node> [--depth 2] [--relation R] [--graph .]  # Blast radius - what breaks if you change this node
astria map [--budget 2000] [--graph .]       # PageRank-ranked repo map with top symbols
astria diagnose [--graph .] [--json]         # Read-only graph health report
astria save-result <question> --answer <text> [--outcome useful|dead_end|corrected]  # Curate a Q/A into graph memory
astria reflect [--graph .]                   # Aggregate memory outcomes into LESSONS.md
astria global add <path> [--as <tag>]        # Merge a repo into the cross-repo global graph
astria global remove <tag> | list | path <A> <B>  # Manage and query the global graph
astria add <url> [--author] [--contributor]     # Fetch arXiv/tweet/webpage/image/PDF into ./raw + update graph
astria add --scip <index.json>                  # Ingest a simplified SCIP JSON index instead of a URL
astria add --postgres <dsn>                     # Introspect a live PostgreSQL schema (requires psql)
astria mcp [--graph .]                             # Run MCP stdio server - query the graph from any AI agent
astria tree [--out tree.html] [--max-children 40] # Collapsible filesystem tree of all symbols (HTML)
astria wiki [--out .astria/wiki] [--max-nodes 25] [--graph .]  # Wikipedia-style markdown wiki (agent-crawlable)
astria prs [20] [--conflicts] [--graph .]         # Map open PRs onto the graph - impact + merge-order risk
astria stats [--graph .]                     # Node/edge/community counts
astria status [--graph .]                    # Graph health and staleness
astria migrate [--graph .]                   # One-time pre-1.0 layout migration (.graphify -> .astria)
astria export [--graph .] [--out graph.json] [--format json|html|graphml|cypher] [--mode standard|large] # Export graph; HTML defaults to standard
astria cluster-only <path>                   # Re-cluster + analyze + report without re-extracting
astria merge <pathA> <pathB> <outPath>       # Merge two graphs
astria diff <pathA> <pathB>                  # Compare two graphs
astria history [--limit 20] [--graph .]      # Show recent query history
astria install [--platform claude]           # Install skill files for AI coding assistants
astria uninstall [--platform claude]         # Uninstall skill files
astria hook install|uninstall|status         # Git hook management
```

Supported platforms for `install`: `claude`, `codex`, `gemini`, `cursor`, `copilot`, `aider`, `opencode`, `kiro`, `trae`, `zcode`.

Running `astria run .` creates `.astria/` with:

- `db.sqlite` — the graph database
- `graph.json` — full graph export
- `graph_report.md` — report with hub nodes, communities, surprising connections

### HTML visualization modes

Use `--format html` to create an interactive vis-network graph view. HTML export applies a 5,000-node safety limit:

```bash
astria export --graph . --format html --out graph-view.html
```

The default `--mode standard` exports the full interactive graph when it contains at most 5,000 nodes and fails with an actionable message for larger graphs. For larger repositories, explicitly opt into the optimized large-graph viewer:

```bash
astria export --graph . --format html --mode large --out graph-view.html
```

Large mode precomputes node positions, disables physics, shows the highest-degree nodes first, supports debounced search and a “Show all nodes” toggle, caps the community legend, and disables expensive edge arrows for very large graphs. JSON and GraphML exports are unaffected by `--mode`.

`--format cypher` writes an idempotent Neo4j import script (MERGE statements — safe to re-run):

```bash
astria export --graph . --format cypher --out astria.cypher
cypher-shell -u neo4j -p <password> -f astria.cypher
```

### Learning from usage

The graph compounds in value as you query it. Every query records which (seed, discovered) node pairs its traversal connected; when the same pair recurs across **at least 2 distinct questions with 3+ total hits**, the next `run`/`update` promotes it to a `learned` edge (INFERRED, hits-scored, provenance `query_history`). Learned edges flow into clustering, analysis, and every export — the graph remembers which connections you actually keep asking about. High-fidelity traversals (`--detail high`) can filter them like any INFERRED fact.

### Token reduction benchmark

Every `run` and `update` prints an honest cost measurement: corpus tokens (the real file sizes from the manifest) versus the tokens a graph query actually returns, sampled over five representative questions. On this repository at v0.8.0: ~333,000 corpus tokens vs ~3,000 per query — **110× fewer tokens per query**; on the Python Graphify codebase: **52×**. On tiny corpora it will honestly report <1x; there the graph's value is structure, not compression, and the output says so. Numbers vary per run and corpus — [methodology, head-to-head, and the embedding experiment](worked/head-to-head/).

### Wiki export

`astria wiki` writes a Wikipedia-style markdown wiki into `.astria/wiki/`: an `index.md` entry point, one article per community (key concepts ranked by connections, cross-community links, source files, EXTRACTED/INFERRED/AMBIGUOUS audit trail), and one article per god node (signature, connections grouped by relation). Articles cross-link with relative markdown links, so any agent — or GitHub, or Obsidian — can navigate the graph by reading files instead of running queries:

```bash
astria run . --wiki          # build graph + wiki in one step
astria wiki --graph .        # (re)generate the wiki any time
astria wiki --out docs/wiki  # export into docs/ for GitHub
```

`update` regenerates an existing wiki automatically, so it never drifts stale.

`--format obsidian` writes an Obsidian vault instead: one note per node with `astria/*` + community tags and `[[wikilinks]]` to neighbors, `_COMMUNITY_*.md` overview notes, and a `astria.canvas` (communities as colored groups, nodes as cards). Open the output directory as a vault in Obsidian:

```bash
astria wiki --format obsidian --out my-vault
```

### .astriaignore

Place a `.astriaignore` file in your project root (gitignore syntax) to exclude files from the graph.

## Semantic enrichment

Two independent semantic layers, both optional:

**Local embeddings (no API key)** — `run --embed` downloads a small local model once (~90 MB, then offline forever) and computes vector embeddings for every node. This adds:

- `similar_to` edges (INFERRED, cosine-scored) linking semantically related symbols across files — they flow into clustering, surprising connections, and every export
- embedding-backed query recall: `query` merges semantic candidates with token matching, so conceptual questions with zero string overlap still find their symbols

Once embeddings exist, every `run`/`update` refreshes them incrementally (offline — the refresh never downloads), and `query` picks them up automatically. Override the model cache location with `ASTRIA_EMBED_CACHE_DIR` (legacy `GRAPHIFY_EMBED_CACHE_DIR` accepted).

**LLM enrichment** — set any LLM backend and the pipeline enriches docs, papers, and images into concept nodes automatically:

| Backend | Env vars | Vision |
|---------|----------|--------|
| Anthropic Claude (default) | `ASTRIA_LLM_API_KEY` | ✓ |
| OpenAI-compatible (OpenAI, DeepSeek, Ollama, LM Studio, custom) | `ASTRIA_LLM_BASE_URL` (or `OPENAI_BASE_URL`) + `ASTRIA_LLM_API_KEY`/`OPENAI_API_KEY` | ✓ |
| Google Gemini | `GEMINI_API_KEY` or `GOOGLE_API_KEY` | ✓ |

`ASTRIA_LLM_BACKEND` selects explicitly; `ASTRIA_LLM_MODEL` overrides (legacy `GRAPHIFY_*` names still honored) the model. Per-run: `astria run . --backend openai --model gpt-4o-mini`. Images (png/jpg/webp/gif, ≤5 MB) go through each backend's vision API.

## Architecture

Rust workspace with 14 crates + Node.js CLI:

```
crates/
  astria-core/      Types, error, SQLite schema + migrations, path validation, sensitive-path denylist
  astria-paths/     Path normalization, .astria directory management
  astria-detect/    File discovery, classification, incremental change detection
  astria-extract/   Tree-sitter AST extraction (21 languages)
  astria-build/     Merge extractions into SQLite graph, entity dedup (MinHash + Jaro-Winkler)
  astria-cluster/   Deterministic label propagation community detection
  astria-analyze/   God nodes, surprising connections, blast radius
  astria-query/     Query engine: BFS/DFS (optionally directed), shortest path, explain
  astria-mcp/       MCP stdio server exposing the graph to AI agents
  astria-report/    Markdown report generation
  astria-semantic/  LLM semantic extraction (Claude / OpenAI-compatible / Gemini), with vision
  astria-ingest/    URL ingestion (arXiv/tweet/webpage/image) with SSRF protection
  astria-pdf/       PDF text extraction
  astria-napi/      napi-rs bindings, pipeline orchestration, merge/diff, JSON/HTML/GraphML/tree export
packages/
  astria-cli/       Node.js CLI (commander.js)
```

Pipeline: `detect() → extract() → enrich_with_semantics() → build() → dedup_nodes() → cluster() → analyze() → report()`

Each stage is a pure function in its own crate; semantic enrichment is optional and activates when an LLM backend is configured. SQLite is the persistence layer (extraction cache, file manifest, graph storage, pipeline runs, query history). petgraph provides in-memory algorithms (BFS/DFS, label propagation, shortest path).

Design docs: [design spec](docs/superpowers/specs/2026-04-30-astria-rewrite-design.md), [implementation plan](docs/superpowers/plans/2026-04-30-astria-implementation.md).

## Build from source

```bash
# Build Rust core
cargo build --release

# Build Node.js CLI
cd packages/astria-cli && npm run build
```

Requires Rust 2021 edition (Rust 1.56+) and Node.js >= 20.

## Test

```bash
cargo test  # All Rust crates: unit tests + end-to-end pipeline integration tests
cd packages/astria-cli && npm run build && npm test  # CLI tests + end-to-end test of the compiled binary
```

Rust crates have unit tests using in-memory SQLite (`open_db_in_memory()`) and `tempfile` for filesystem fixtures, plus integration tests in `crates/astria-napi/tests/` that run the full pipeline over language fixtures. The CLI package has structure tests against the real Commander program, install/hook tests, and an end-to-end test that spawns the compiled CLI against a fixture project (skips automatically if `dist/` hasn't been built).

## Language support

Python, JavaScript, TypeScript, Rust, Go, Java, C, C++, Ruby, Swift, Kotlin, Scala, PHP, C#, Lua, Haskell, Elixir, Bash, Dart, Zig, CSS — via tree-sitter grammars.

Each language has its own config module in `crates/astria-extract/src/langs/`. Adding a new language means adding a new file there and registering it in `langs/mod.rs`.

## License

MIT
