<div align="center">

# astria

**Knowledge graph builder for codebases**

[![CI](https://github.com/Nodesify/astria/actions/workflows/ci.yml/badge.svg)](https://github.com/Nodesify/astria/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/@nodesify/astria)](https://www.npmjs.com/package/@nodesify/astria)
[![npm downloads](https://img.shields.io/npm/dm/@nodesify/astria)](https://www.npmjs.com/package/@nodesify/astria)
[![docs](https://img.shields.io/badge/docs-latest-blue)](https://nodesify.github.io/astria/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Node](https://img.shields.io/badge/node-22-339933?logo=nodedotjs&logoColor=white)](https://nodejs.org/)
[![Ask DeepWiki](.github/assets/deepwiki-badge.svg)](https://deepwiki.com/Nodesify/astria)

[Docs](https://nodesify.github.io/astria/) | [Getting started](https://nodesify.github.io/astria/docs/getting-started) | [CLI Reference](https://nodesify.github.io/astria/docs/reference/cli) | [Architecture](ARCHITECTURE.md) | [Worked examples](worked/) | [Changelog](CHANGELOG.md) | [Release notes](https://nodesify.github.io/astria/blog)

</div>

Understand a codebase before you touch it. `astria` turns any folder into a queryable knowledge graph — deterministic AST extraction in Rust, optional local-embedding semantics, zero API keys, everything on your machine.

astria is inspired by the Python [Graphify](https://github.com/safishamsi/graphify) project's core idea — turn a corpus into a queryable knowledge graph — but it is an independent, from-scratch implementation: a deterministic, offline-first Rust/tree-sitter pipeline, not a fork or a port. astria is not affiliated with, sponsored by, or endorsed by the Graphify project or Graphify Labs.

Find source-grounded code context before changing a repository. Astria retrieves symbols, file locations, and relationships from the latest committed graph snapshot through the same CLI and MCP query engine. Updates reuse cached AST extraction and reconcile references across the current corpus; inferred name matches remain distinct from declared facts.

Retrieval quality and delivered context cost are measured separately. The [paired benchmark methodology](scripts/bench/paired/README.md) records file recall, source-grounded symbol checks, exact response tokens and failures. The [latest results](website/docs/explanation/retrieval-validation.md) include improvements and regressions. Historical full-corpus/query token ratios are size diagnostics, not measured savings over targeted source search. A measured first-shot comparison against a deterministic question-derived rg-plus-reads baseline is checked in at [worked/external-baseline/](worked/external-baseline/): ~207 delivered tokens per question put the defining file first for 8/8 external questions, while the baseline's 4,000-token responses never ranked it first — quality per delivered token against a single-pass search floor, not a claim about expert iterative search.

Code definitions retain their scoped identities during deduplication. Extraction includes assigned JS/TS functions and Python implementations behind overload declarations. Query text uses exact `o200k_base` budgets shared by CLI and MCP. After upgrading, run `astria update .` to refresh extraction and restore previously merged definitions; start fresh pagination because cursors now count both node and edge records.

Three things a folder full of files can't give you:

1. **Structure that survives the session** — hub files, god nodes, communities, and the blast radius of any change, stored in SQLite and refreshed incrementally as code changes.
2. **An honest audit trail** — every edge is labeled EXTRACTED / INFERRED / SEMANTIC / AMBIGUOUS with a numeric confidence score. You always know what was found in the source versus deduced versus LLM-enriched, and `--detail high` filters to only declared facts ([graph model](https://nodesify.github.io/astria/docs/reference/graph-model)).
3. **Answers for agents and humans** — query it from the CLI, from any AI agent via MCP, or just read the exported markdown wiki with plain file links.

[Worked examples with honest reviews](worked/) — the tool run on itself, including what the graph got *wrong* — plus a [head-to-head benchmark](worked/head-to-head/) against the Python Graphify project that inspired it, run on the same corpus. The full measurement stack — shared-tokenizer token parity, a golden-QA retrieval-quality harness (recall@k / MRR), blind LLM judging, and a LoCoMo memory adapter — lives in [`scripts/bench/`](scripts/bench/).

## Quick start

```bash
npm install -g @nodesify/astria
```

Requires no Rust toolchain — ships prebuilt native binaries via napi-rs.

Just want the agent skill, no CLI? `npx skills add Nodesify/astria` installs the graph-first skill from [skills.sh](https://skills.sh) - it answers from an existing `.astria/` graph as plain files and, when graph commands are needed, offers the install above (never without asking).

On Claude Code? One plugin bundles the MCP server, the skill, `/astria` + `/astria-risk` commands, and the `astria-architect` subagent: `/plugin marketplace add Nodesify/astria` then `/plugin install astria@nodesify`.

macOS/Linux without npm? `brew install nodesify/tap/astria` ([tap](https://github.com/Nodesify/homebrew-tap)).

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
> Legacy configuration variables remain supported where documented. LLM activation requires `--backend` or `ASTRIA_LLM_BACKEND`; `GRAPHIFY_LLM_BACKEND` does not opt in.

## Documentation

Full docs live at [nodesify.github.io/astria](https://nodesify.github.io/astria/) — versioned per release, with a `Next` page tracking unreleased work.

| | |
|---|---|
| **Getting started** | [Install and first graph](https://nodesify.github.io/astria/docs/getting-started) |
| **Guides** | [Agent integration (MCP + install)](https://nodesify.github.io/astria/docs/guides/mcp-and-agents) · [Wiki and exports](https://nodesify.github.io/astria/docs/guides/wiki-and-exports) · [Semantic enrichment](https://nodesify.github.io/astria/docs/guides/semantic-enrichment) · [Global graph](https://nodesify.github.io/astria/docs/guides/global-graph) · [Memory and learning](https://nodesify.github.io/astria/docs/guides/memory-and-learning) |
| **Reference** | [CLI](https://nodesify.github.io/astria/docs/reference/cli) · [MCP tools](https://nodesify.github.io/astria/docs/reference/mcp-tools) · [Environment variables](https://nodesify.github.io/astria/docs/reference/env-vars) · [Graph model](https://nodesify.github.io/astria/docs/reference/graph-model) · [The .astria directory](https://nodesify.github.io/astria/docs/reference/directory-layout) · [Language support](https://nodesify.github.io/astria/docs/reference/language-support) · [Troubleshooting](https://nodesify.github.io/astria/docs/reference/troubleshooting) |
| **Explanation** | [Architecture](https://nodesify.github.io/astria/docs/explanation/architecture) · [Benchmarks and evidence](https://nodesify.github.io/astria/docs/explanation/benchmarks) · [Retrieval validation](https://nodesify.github.io/astria/docs/explanation/retrieval-validation) |

### Feature highlights

- **Query it three ways** — CLI (`query`, `explain`, `path`, `affected`, `map`), an MCP server for AI agents, or an exported [markdown wiki](https://nodesify.github.io/astria/docs/guides/wiki-and-exports) any agent (or human) can crawl
- **Local embeddings, no API key** — `run --embed` adds `similar_to` edges and semantic query recall ([semantic enrichment guide](https://nodesify.github.io/astria/docs/guides/semantic-enrichment))
- **Optional LLM enrichment, measured and cached** — Claude, any OpenAI-compatible endpoint, or Gemini, with an optional Jev judge layer on top of any of them (`--judge jev`): the judge re-judges relations and node types from the schema allowlists, gives every semantic edge a calibrated confidence score, and can batch-gate trivial files and re-rank suggested questions before they cost engine calls. Vision included for images; per-run with `--backend`/`--model`/`--judge` or env vars. Thematic community naming (`run --label-communities`, one call per *changed* community) and a `--deep` concept-linking tier (one call per changed file) are content-hash cached, so unchanged inputs and effective configuration can reuse cached output. Backend selection is explicit; credentials alone never activate enrichment. Every response's usage block is counted — the run summary prints API calls and input/output tokens, and `ASTRIA_LLM_BUDGET` caps the spend ([semantic enrichment guide](https://nodesify.github.io/astria/docs/guides/semantic-enrichment))
- **Cross-repo global graph** — merge many repos into one queryable store at `~/.astria/global.db` ([global graph guide](https://nodesify.github.io/astria/docs/guides/global-graph))
- **The graph compounds with use** — repeated queries become `learned` edges; curated Q/A memory via `save-result`/`reflect` ([memory and learning](https://nodesify.github.io/astria/docs/guides/memory-and-learning))
- **Interactive HTML viewer, SVG, and live Neo4j** — physics-free large-graph HTML mode, deterministic community-arc SVG for Notion/GitHub embedding, an idempotent Cypher script, or a direct Bolt push into a running Neo4j — hand-rolled protocol client, zero driver dependencies ([wiki and exports](https://nodesify.github.io/astria/docs/guides/wiki-and-exports))
- **The analyst built in** — `astria health` scores unreachable-symbol candidates, circular file dependencies, hub concentration, and staleness into one 0-100 report (also an MCP tool); `astria risk` maps the current git diff onto the graph and renders the blast radius as a PR-ready risk report ([guides](https://nodesify.github.io/astria/docs/guides/mcp-and-agents))
- **Retrieval measurement** — the [quality harness](scripts/bench/quality/README.md) separates file retrieval accuracy from delivered context tokens, includes failed queries in its denominator, and provides an opt-in external corpus comparison.
- **Measured quality, not just cost** — a golden-QA harness scores recall@k / MRR of real query answers, a blind LLM judge grades astria against the original on the same corpus, and a LoCoMo adapter runs the memory-retrieval protocol the original publishes ([benchmarks](https://nodesify.github.io/astria/docs/explanation/benchmarks), [harness](scripts/bench/))
- **10 MCP tools** — query_graph, repo_map, explain, get_neighbors, shortest_path, affected, god_nodes, list_communities, graph_stats, health ([MCP tools reference](https://nodesify.github.io/astria/docs/reference/mcp-tools))

- **Agent skill on skills.sh** - `npx skills add Nodesify/astria` installs the graph-first skill on its own; it detects the CLI and guides install on first use ([skill file](https://github.com/Nodesify/astria/blob/main/skills/astria/SKILL.md))

- **Claude Code plugin & official MCP Registry listing** - `/plugin marketplace add Nodesify/astria` installs the MCP server, skill, commands, and subagent as one plugin; the server is published to the official MCP Registry ([server.json](server.json))

## Architecture

Rust workspace with 16 crates + Node.js CLI:

```
crates/
  astria-bolt/      Bolt client for live Neo4j exports
  astria-core/      Types, error, SQLite schema + migrations, path validation, sensitive-path denylist
  astria-paths/     Path normalization, .astria directory management
  astria-detect/    File discovery, classification, incremental change detection
  astria-extract/   Tree-sitter AST extraction (25 languages)
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

Pipeline: `detect() → extract() → enrich_with_semantics() → build() → dedup_nodes() → embed() (optional --embed) → cluster() → analyze() → report()`

Pipeline stages separate extraction, persistence, and derived outputs. Semantic enrichment requires explicit `--backend` or `ASTRIA_LLM_BACKEND` selection; credentials alone do not activate it. SQLite is the persistence layer (extraction cache, file manifest, graph storage, pipeline runs, query history). petgraph provides in-memory algorithms (BFS/DFS, label propagation, shortest path).

## Build from source

```bash
# Build Rust core
cargo build --release

# Copy the native library for the source CLI (Linux)
mkdir -p packages/astria-cli/dist
cp target/release/libastria_napi.so packages/astria-cli/dist/astria.node

# Build Node.js CLI
cd packages/astria-cli && npm ci && npm run build
```

See [Contributing](CONTRIBUTING.md#development-setup) for macOS and Windows native artifact paths.

Requires Rust 1.88+ (declared as `rust-version` in the workspace) and Node.js >= 22.

## Test

```bash
cargo test  # All Rust crates: unit tests + end-to-end pipeline integration tests
cd packages/astria-cli && npm run build && npm test  # CLI tests + end-to-end test of the compiled binary
```

Rust crates have unit tests using in-memory SQLite (`open_db_in_memory()`) and `tempfile` for filesystem fixtures, plus integration tests in `crates/astria-napi/tests/` that run the full pipeline over language fixtures. The CLI package has structure tests against the real Commander program, install/hook tests, and an end-to-end test that spawns the compiled CLI against a fixture project (skips automatically if `dist/` hasn't been built).

## Language support

Python, JavaScript, TypeScript, Rust, Go, Java, C, C++, Ruby, Swift, Kotlin, Scala, PHP, C#, Lua, Haskell, Elixir, Bash, Dart, Zig, CSS, Terraform/HCL, PowerShell, Verilog/SystemVerilog, Metal — via tree-sitter grammars.

Each language has its own config module in `crates/astria-extract/src/langs/`. Adding a new language means adding a new file there and registering it in `langs/mod.rs` — [language support docs](https://nodesify.github.io/astria/docs/reference/language-support).

## License

MIT — see [LICENSE](LICENSE).

Contributions are welcome and accepted under the [Contributor License Agreement](CLA.md) — see [CONTRIBUTING.md](CONTRIBUTING.md) to get started.
