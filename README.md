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

[Docs](https://nodesify.github.io/astria/) | [Getting started](https://nodesify.github.io/astria/docs/getting-started) | [CLI Reference](https://nodesify.github.io/astria/docs/reference/cli) | [Architecture](ARCHITECTURE.md) | [Worked examples](worked/)

</div>

Understand a codebase before you touch it. `astria` turns any folder into a queryable knowledge graph — deterministic AST extraction in Rust, optional local-embedding semantics, zero API keys, everything on your machine.

astria is inspired by the Python [Graphify](https://github.com/safishamsi/graphify) project's core idea — turn a corpus into a queryable knowledge graph — but it is an independent, from-scratch implementation: a deterministic, offline-first Rust/tree-sitter pipeline, not a fork or a port. astria is not affiliated with, sponsored by, or endorsed by the Graphify project or Graphify Labs.

You drop into an unfamiliar repo and need to know: what is load-bearing here, what breaks if I change this, where does auth live, how do these two modules connect. Reading everything costs the whole context window. The graph answers in ~3,000 tokens — **measured** at **50–110× fewer tokens per query** on real repos (printed honestly after every run, computed from real file sizes vs actual query output — [methodology and head-to-head](https://nodesify.github.io/astria/docs/explanation/benchmarks)).

Three things a folder full of files can't give you:

1. **Structure that survives the session** — hub files, god nodes, communities, and the blast radius of any change, stored in SQLite and refreshed incrementally as code changes.
2. **An honest audit trail** — every edge is labeled EXTRACTED / INFERRED / AMBIGUOUS with a numeric confidence score. You always know what was found in the source versus deduced, and `--detail high` filters to only declared facts ([graph model](https://nodesify.github.io/astria/docs/reference/graph-model)).
3. **Answers for agents and humans** — query it from the CLI, from any AI agent via MCP, or just read the exported markdown wiki with plain file links.

[Worked examples with honest reviews](worked/) — the tool run on itself, including what the graph got *wrong* — plus a [head-to-head benchmark](worked/head-to-head/) against the Python Graphify project that inspired it, run on the same corpus. The full measurement stack — shared-tokenizer token parity, a golden-QA retrieval-quality harness (recall@k / MRR), blind LLM judging, and a LoCoMo memory adapter — lives in [`scripts/bench/`](scripts/bench/).

## Quick start

```bash
npm install -g @nodesify/astria
```

Requires no Rust toolchain — ships prebuilt native binaries via napi-rs.

Just want the agent skill, no CLI? `npx skills add Nodesify/astria` installs the graph-first skill from [skills.sh](https://skills.sh) - it answers from an existing `.astria/` graph as plain files and, when graph commands are needed, offers the install above (never without asking).

```bash
astria run .                                  # build the graph (creates .astria/)
astria query "how does authentication work"   # ask the graph a question
astria map                                    # PageRank-ranked repo map for orientation
astria affected <node>                        # what breaks if you change this
```

Exclude files with a `.astriaignore` file in the project root (gitignore syntax). Everything astria writes lives in plain files under `.astria/` — [the full layout](https://nodesify.github.io/astria/docs/reference/directory-layout).

> **Migrating from `@nodesify/graphify`?** 1.0 is a rebrand: the binary is `astria`, the npm package is `@nodesify/astria`, and graphs live in `.astria/` instead of `.graphify/`. Run once after installing:
>
> ```bash
> astria migrate          # renames .graphify/ -> .astria/ and the global store
> astria install          # refreshes AI-tool skills/hooks (also cleans the old graphify entries)
> ```
>
> `GRAPHIFY_*` environment variables keep working; `ASTRIA_*` takes precedence.

## Documentation

Full docs live at [nodesify.github.io/astria](https://nodesify.github.io/astria/) — versioned per release, with a `Next` page tracking unreleased work.

| | |
|---|---|
| **Getting started** | [Install and first graph](https://nodesify.github.io/astria/docs/getting-started) |
| **Guides** | [Agent integration (MCP + install)](https://nodesify.github.io/astria/docs/guides/mcp-and-agents) · [Wiki and exports](https://nodesify.github.io/astria/docs/guides/wiki-and-exports) · [Semantic enrichment](https://nodesify.github.io/astria/docs/guides/semantic-enrichment) · [Global graph](https://nodesify.github.io/astria/docs/guides/global-graph) · [Memory and learning](https://nodesify.github.io/astria/docs/guides/memory-and-learning) |
| **Reference** | [CLI](https://nodesify.github.io/astria/docs/reference/cli) · [MCP tools](https://nodesify.github.io/astria/docs/reference/mcp-tools) · [Environment variables](https://nodesify.github.io/astria/docs/reference/env-vars) · [Graph model](https://nodesify.github.io/astria/docs/reference/graph-model) · [The .astria directory](https://nodesify.github.io/astria/docs/reference/directory-layout) · [Language support](https://nodesify.github.io/astria/docs/reference/language-support) · [Troubleshooting](https://nodesify.github.io/astria/docs/reference/troubleshooting) |
| **Explanation** | [Architecture](https://nodesify.github.io/astria/docs/explanation/architecture) · [Benchmarks and evidence](https://nodesify.github.io/astria/docs/explanation/benchmarks) |

### Feature highlights

- **Query it three ways** — CLI (`query`, `explain`, `path`, `affected`, `map`), an MCP server for AI agents, or an exported [markdown wiki](https://nodesify.github.io/astria/docs/guides/wiki-and-exports) any agent (or human) can crawl
- **Local embeddings, no API key** — `run --embed` adds `similar_to` edges and semantic query recall ([semantic enrichment guide](https://nodesify.github.io/astria/docs/guides/semantic-enrichment))
- **Optional LLM enrichment** — Claude, any OpenAI-compatible endpoint, or Gemini; vision included for images; per-run with `--backend`/`--model` or env vars
- **Cross-repo global graph** — merge many repos into one queryable store at `~/.astria/global.db` ([global graph guide](https://nodesify.github.io/astria/docs/guides/global-graph))
- **The graph compounds with use** — repeated queries become `learned` edges; curated Q/A memory via `save-result`/`reflect` ([memory and learning](https://nodesify.github.io/astria/docs/guides/memory-and-learning))
- **Interactive HTML viewer + Neo4j export** — physics-free large-graph mode beyond the 5,000-node safety cap, idempotent Cypher script ([wiki and exports](https://nodesify.github.io/astria/docs/guides/wiki-and-exports))
- **Honest token math** — every run prints measured corpus-vs-query tokens: 110× on this repo. The printed estimate names its heuristic; the published snapshot also counts both tools with one shared tokenizer so absolute numbers are directly comparable ([benchmarks](https://nodesify.github.io/astria/docs/explanation/benchmarks))
- **Measured quality, not just cost** — a golden-QA harness scores recall@k / MRR of real query answers, a blind LLM judge grades astria against the original on the same corpus, and a LoCoMo adapter runs the memory-retrieval protocol the original publishes ([benchmarks](https://nodesify.github.io/astria/docs/explanation/benchmarks), [harness](scripts/bench/))
- **9 MCP tools** — query_graph, repo_map, explain, get_neighbors, shortest_path, affected, god_nodes, list_communities, graph_stats ([MCP tools reference](https://nodesify.github.io/astria/docs/reference/mcp-tools))

- **Agent skill on skills.sh** - `npx skills add Nodesify/astria` installs the graph-first skill on its own; it detects the CLI and guides install on first use ([skill file](https://github.com/Nodesify/astria/blob/main/skills/astria/SKILL.md))

## What's new in 1.0.0

The rebrand release — everything is now astria: the binary, the npm package, the `.astria/` graph directory, `ASTRIA_*` env vars, and the installed skill files. `astria migrate` moves pre-1.0 layouts.

- **Hypergraph, deterministically** — n-ary `hyperedges` (community `participate_in` groups, `shares_reference` literal groups) produced without an LLM; consumed by graph.json, report, wiki, HTML hulls, and `explain`. Graphify's hyperedges are LLM-produced; ours are local and reproducible.
- **Cross-repo global graph** — `~/.astria/global.db`: `global add/remove/list/path`, repo-tag prefixed merging that unifies external symbols across repos, `same_type_as` type edges, cross-repo call resolution (fail closed on ambiguity), `run --global --as <tag>`, and `query/explain/path --graph` against the merged store.
- **Graph health + feedback loop** — `diagnose` (read-only health report, `--json`), `save-result`/`reflect` curated memory (`.astria/memory/` → graph nodes → `LESSONS.md`) alongside automatic learned edges, build-time validation, JSONL query log (`ASTRIA_QUERY_LOG`), and always-on instruction blocks in `AGENTS.md`/`CLAUDE.md`.
- **Ingest breadth (offline-first)** — Cargo workspace + path-dep topology (auto), `.mcp.json`/`mcp_servers.json`/`claude_desktop_config.json` (env names only, never values), `add --scip <index.json>`, `add --postgres <dsn>` (read-only introspection, requires `psql`), and transcript sidecars (`.astria/transcripts/*.txt|md`).
- **SSRF-hardened URL ingestion** — `add <url>` validates every redirect hop (auto-follow is off), DNS-resolves each host and blocks loopback/private/CGNAT/link-local addresses (IPv4 and IPv6, incl. mapped forms), and slugifies downloaded filenames so a hostile URL segment cannot write outside `raw/`. The HTML export renders labels as text (no HTML interpolation), a plain-`http` LLM base URL with an API key warns, and CI audits npm dependencies alongside the existing Rust advisory check.

Full release history: [release notes](https://nodesify.github.io/astria/blog).

## Architecture

Rust workspace with 15 crates + Node.js CLI:

```
crates/
  astria-core/      Types, error, SQLite schema + migrations, path validation, sensitive-path denylist
  astria-paths/     Path normalization, .astria directory management
  astria-detect/    File discovery, classification, incremental change detection
  astria-extract/   Tree-sitter AST extraction (21 languages)
  astria-embed/     Local embeddings (fastembed/ONNX) — similar_to edges, semantic query recall
  astria-build/     Merge extractions into SQLite graph, entity dedup (MinHash + Jaro-Winkler)
  astria-cluster/   Deterministic label propagation community detection
  astria-analyze/   God nodes, surprising connections, blast radius
  astria-query/     Query engine: BFS/DFS (optionally directed), shortest path, explain
  astria-mcp/       MCP stdio server exposing the graph to AI agents
  astria-report/    Markdown report generation
  astria-semantic/  LLM semantic extraction (Claude / OpenAI-compatible / Gemini), with vision
  astria-ingest/    URL ingestion (arXiv/tweet/webpage/image), SCIP + Postgres intake, SSRF protection
  astria-pdf/       PDF text extraction
  astria-napi/      napi-rs bindings, pipeline orchestration, merge/diff, JSON/HTML/GraphML/tree export
packages/
  astria-cli/       Node.js CLI (commander.js)
```

Pipeline: `detect() → extract() → enrich_with_semantics() → build() → dedup_nodes() → cluster() → analyze() → report()`

Each stage is a pure function in its own crate; semantic enrichment is optional and activates when an LLM backend is configured. SQLite is the persistence layer (extraction cache, file manifest, graph storage, pipeline runs, query history). petgraph provides in-memory algorithms (BFS/DFS, label propagation, shortest path).

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

Each language has its own config module in `crates/astria-extract/src/langs/`. Adding a new language means adding a new file there and registering it in `langs/mod.rs` — [language support docs](https://nodesify.github.io/astria/docs/reference/language-support).

## License

MIT — see [LICENSE](LICENSE).

Contributions are welcome and accepted under the [Contributor License Agreement](CLA.md) — see [CONTRIBUTING.md](CONTRIBUTING.md) to get started.
