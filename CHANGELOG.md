# Changelog

All notable changes to astria are documented here. Release notes with full
narrative live on the [docs site blog](https://nodesify.github.io/astria/blog);
this file is the per-version summary.

## [1.0.4] — 2026-09-26

### Changed
- **Query answers are relevance-ranked, not hub-ranked.** Answer nodes
  order by question-match score first, then traversal distance to the
  matching seeds, then degree — pure degree ordering buried the files a
  question was about beneath graph-wide hubs. Question words ("where",
  "does", "what"…) are filtered as stopwords before scoring, and code
  symbols outrank prose/stub nodes on equal term evidence. Measured with
  the golden-QA harness on this repo: MRR 0.058 → 0.533, recall@5
  2.9% → 65.7%, recall@10 11.4% → 80%.
- `stats` now prints a node-type breakdown (code/stub/document/…), keeping
  `status` focused on graph health and staleness.
- `save-result` without `--outcome` prints how to record one — reflect
  skips outcome-less entries.

### Fixed
- `path` fed exact qualified ids to fuzzy scoring, so endpoints silently
  resolved to unrelated nodes ("src_lib::fetch_bytes" top-ranked an
  "as_bytes" node). Exact ids now win over scoring, with the same
  stub-does-not-shadow rule as affected/explain; scoring stays as the
  fallback for natural-language endpoints.
- Node-id prefixes were derived from the CWD-joined path, so identical
  content at different roots (relocated checkouts, clones) churned every
  id and `diff`/`merge` mismatched whole graphs. Prefixes now come from
  the path relative to the scanned root.

## [1.0.3] — 2026-09-26

### Fixed
- `affected`/`explain` resolved same-named code symbols to unrelated
  nodes. Three defects compounded: entity dedup merged `validate_url()`
  into the `validate.rs` file node (Jaro-Winkler on normalized labels is
  shape-blind), INFERRED call edges carried bare-name targets that
  collided with stubs from unrelated files, and seed resolution returned
  stubs before real definitions. Dedup now refuses cross-shape merges
  (symbol / file / free-form), the build resolves bare call targets to
  same-file definitions, and seed lookup prefers definitions over stubs.
  On this repo the `validate_url` blast radius went from 1 wrong node to
  7 (real callers plus the redirect path).

## [1.0.2] — 2026-09-26

### Added
- Benchmark quality stack under `scripts/bench/`: a shared o200k_base
  tokenizer for the snapshot (`token_parity` block — absolute corpus/query
  tokens now directly comparable across both tools), a golden-QA
  retrieval-quality harness (recall@k / MRR over 35 grounded questions,
  run in CI as a non-blocking job), a blind promptfoo judging config
  (astria vs the original on the same corpus, LLM rubric), and a LoCoMo
  memory-benchmark adapter (transcript sidecars, 1,977 evidence-backed
  QA pairs) for apples-to-apples recall measurement
- The printed token benchmark now names its estimator (4 chars/token
  heuristic) and points at the snapshot's exact shared-tokenizer counts
- Canonical agent skill at `skills/astria/SKILL.md`, indexed on skills.sh
  (`npx skills add Nodesify/astria`) - usable without the CLI installed: it
  reads an existing `.astria/` graph as plain files and guides a one-command
  CLI install (with user consent) when graph commands are needed
- Project-scoped `.mcp.json` is now committed so fresh clones register the
  astria MCP server without running `astria install`

### Changed
- Repo hygiene: the canonical agent skill now lives only at
  `skills/astria/SKILL.md` — the per-tool artifacts `astria install`
  generates (`CLAUDE.md`, `GEMINI.md`, `.agents/`, `.opencode/`) are no
  longer committed (installed copies had already drifted from the canonical
  skill); the multi-MB `graph.json` worked-example graphs are no longer
  stored (regenerable via the documented reproduce commands) and the
  canonical quality results moved to `worked/astria/quality-results.json`;
  workspace crates are marked `publish = false` (distribution is npm-only);
  the root `tests/fixtures/` language samples moved into
  `crates/astria-napi/tests/fixtures/` beside the integration tests that
  use them; a pull-request template is added.

### Removed
- The six pre-1.0 `@nodesify/graphify*` npm packages (the old CLI and its
  five platform binaries) are fully unpublished — the names are gone from
  the registry, verified 404 on 2026-09-26. Installs pinned to the old
  names now fail with a hard 404 rather than showing a deprecation
  pointer; the migration path is `npm i -g @nodesify/astria`, then
  `astria migrate` and `astria install` (also in the README, the CLI
  README, and the 1.0 blog post).

## [1.0.1]
- Release-infrastructure fixes only (prod-environment publishing, idempotent
  publish reruns, full npm debug log on failure); no product changes.

## [1.0.0] — the astria rebrand

Everything is now astria: the binary, the npm package (`@nodesify/astria`),
the `.astria/` graph directory, `ASTRIA_*` environment variables, and the
installed skill files. `astria migrate` moves pre-1.0 layouts.

### Added
- **Deterministic hypergraph** — n-ary `hyperedges` (community `participate_in`
  groups, `shares_reference` literal groups) produced without an LLM; consumed
  by graph.json, report, wiki, HTML hulls, and `explain`
- **Cross-repo global graph** — `~/.astria/global.db`: `global add/remove/list/path`,
  repo-tag prefixed merging, `same_type_as` type edges, cross-repo call
  resolution (fail closed on ambiguity)
- **Graph health + feedback loop** — `diagnose`, `save-result`/`reflect`
  curated memory (`LESSONS.md`), build-time validation, JSONL query log,
  always-on instruction blocks in `AGENTS.md`/`CLAUDE.md`
- **Ingest breadth** — Cargo workspace + path-dep topology, MCP config files
  (env names only), `add --scip`, `add --postgres`, transcript sidecars
- **SSRF-hardened URL ingestion** — per-hop redirect validation, private/loopback
  address blocking (IPv4 + IPv6), slugified downloads
- Versioned documentation site with per-release archives

## [0.9.0]
- CI hardening, `zcode` platform support, documentation site reorganization

## [0.8.0]
- Markdown wiki export (`wiki` / `run --wiki`) and Obsidian vault export with canvas
- Local semantic layer (`run --embed`): `similar_to` edges + embedding-backed query recall
- Learning from usage: repeated queries promote recurring node pairs into `learned` edges
- Neo4j export (`export --format cypher`)
- Token benchmark printed after every run
- Security: esbuild advisory pinned out, Windows reserved-name guards, learned-edge promotion hardening

## [0.7.0]
- Edge provenance: `@file:line` anchors on edges and nodes (schema v4)
- Reference nodes for identifier-shaped string literals
- Query output reports graph staleness
- Security hardening: no shell-string exec, literal-allowlist native module loading, install-path containment

## [0.6.1]
- Fixed installs shipping a stale native binary (platform `optionalDependencies` pins now track the package version)

## [0.6.0]
- Safe HTML graph viewer with 5,000-node standard cap and optimized `--mode large` viewer

## [0.5.0]
- Deterministic clustering, directed traversal (`--directed`), fidelity tiers (`--detail high`), continuation cursors
- `astria map` (PageRank-ranked repo map), node signatures, parallel LLM worker pool
- Sensitive-path denylist, minified/vendored asset skip

## [0.4.0]
- `affected` blast-radius analysis, MCP server (9 tools), entity dedup
  (MinHash + Jaro-Winkler), `tree` HTML export, `prs` merge-order risk,
  dependency-manifest nodes

## Earlier releases (0.1.2 – 0.3.0)

See the [GitHub releases page](https://github.com/Nodesify/astria/releases).

[1.0.4]: https://github.com/Nodesify/astria/compare/v1.0.3...v1.0.4
[1.0.3]: https://github.com/Nodesify/astria/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/Nodesify/astria/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/Nodesify/astria/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/Nodesify/astria/compare/v0.9.0...v1.0.0
[0.9.0]: https://github.com/Nodesify/astria/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/Nodesify/astria/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/Nodesify/astria/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/Nodesify/astria/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/Nodesify/astria/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/Nodesify/astria/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/Nodesify/astria/compare/v0.3.0...v0.4.0
