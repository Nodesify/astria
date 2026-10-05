# Changelog

All notable changes to astria are documented here. Release notes with full
narrative live on the [docs site blog](https://nodesify.github.io/astria/blog);
this file is the per-version summary.

## [1.1.0] — 2026-10-06

- Preserve qualified symbol boundaries and import paths when resolving references; ambiguous candidates remain unresolved and inferred bindings retain `RESOLVED` evidence. Extraction cache v15 refreshes previously flattened targets.
- Prefer exact implementation definitions for code-oriented queries, with scope and source metadata contributing to ranking.
- Name communities from source modules and packages; reports separately orient production code, documentation, and tests/benchmarks/examples, with source links and explicit relationship evidence.
- Propagate authoritative database decoding errors in analysis, clustering, hyperedge construction, and reports rather than silently discarding rows.
- Add 24 reserved questions on pinned Requests and Commander sources, a bounded iterative search baseline, and separate retrieval/build cost and provenance records. The new corpus and methods have not been evaluated.
- Simplify the README workflow and align architecture documentation with snapshot caching and reference evidence.
- **Speculative graph nodes no longer claim a source file.** A `stub` or `reference` node exists precisely because no file defined the name, so it has no source locus; it previously borrowed whichever file referenced it first. That fabricated a file location for unresolved globals (a lone `import rusqlite` became `health.rs -> <first file to mention rusqlite>`), producing **743 phantom file-to-file dependencies** on this repo, four file "cycles" that do not exist in the source, and a false `File:` line in `explain`. A whole-graph invariant pass clears stale loci on every run, so an incremental `update` heals graphs built by earlier versions. `explain` now reports `(no source locus — unresolved name, no single owner)` instead of naming a file that does not own the symbol.
- **Fewer false positives from test files.** `is_test_file` did not recognise `__tests__/`, `.test.ts`, or `.spec.ts`, so JS/TS test helpers were scored as production code: on this repo a test-only `assert()` ranked as the #3 hub and `test_hubs_skipped` stayed `0`. Hub detection, dead-code candidacy, and file-cycle inputs now use the same corrected matcher.
- **`astria health --min-score <n>`** exits non-zero when the score is below `n`, so the command works as a CI gate; it previously always exited 0, which made the obvious threshold check silently never fire.
- **`astria merge-gate` checks the extraction ruleset** (`extraction-current`) alongside age and HEAD coverage. A graph reused from an older extraction cache passes both of those while reporting facts the current build would extract differently; the gate now fails and points at `astria update .`.
- **Ranking surfaces ignore non-code nodes.** Hub concentration, `file_cycles`, `report` "Key Files", "surprising connections", and the local embedding layer all exclude speculative nodes, so similarity between two unresolved bare names is no longer presented as structure.
- Ground-truth note: measured on this repository, the fix removes 743 fabricated file dependencies, 4 false cycles, and 1,302 `similar_to` edges that joined two non-symbols; retrieval quality on the golden set is unchanged by these edits (MRR 0.5979 before and after, byte-identical), so this is a correctness and honesty change, not a ranking change.

## [1.0.12] — 2026-10-04


### Project-review backlog — 36 defects + 6 engineering improvements (3 October)

Source-backed review of every workspace crate and the CLI; findings and statuses in `docs/project-review-2026-10-03.md`.

- **Security boundaries** — HTML exports escape every `<` in embedded graph JSON (mixed-case `</SCRIPT>` could terminate the data element); MCP configuration files are excluded from raw LLM enrichment (literal credentials can no longer leave the machine through a remote backend); `bolt+s://`/`bolt+ssc://` now ride rustls with scheme-preserving transport instead of silently downgrading to plaintext TCP; HTTP MCP enforces a whole-request deadline with per-read recomputation and exact loopback matching (`127.attacker.example` is not loopback); `yt-dlp` media downloads are resolved and vetted through the SSRF policy (`-J --simulate`) before any byte moves; URL classification parses host/path instead of substring-matching the whole URL.
- **Graph identity & integrity** — node ids are case-preserving (`Foo`/`foo` stay distinct; matching stays case-folded), with duplicate-id disambiguation rewiring edges to the surviving definition and the extraction cache version advanced to v14 so pre-fix caches invalidate cleanly; automatic global tags check existing names before allocating (no more repo duplication/replacement); merge is atomic — edges reconcile, the database swaps with rollback, artifacts stage as `.new` and flip only after the swap, and the generation is stamped inside the transaction; global replace runs fully transactionally with relation reconciliation after commit.
- **Freshness & publication trust** — every publication mints a generation stamp (`_meta.graph_generation`, `generation.txt`, `_meta` in `graph.json`, report footer) so database, JSON, and report of one build are matchable and snapshot caches key on it; unchanged Google Workspace shortcuts re-check their remote revision instead of trusting shortcut bytes; the merge gate fails on git-detection errors instead of silently skipping its checks.
- **Semantic backend honesty** — token reservations span the whole request+response with guard-based release and per-backend `max_tokens` sharing the same constants (the advertised budget is reserved before the call, not audited after); content needing more than the chunk cap fails loudly (`ASTRIA_LLM_MAX_CHUNKS`) instead of truncating and caching as success; Bedrock `stopReason` is checked before accepting text; derived-text lookup validates against the extraction-hash family (config-hash lookups that never matched what extraction wrote are gone); malformed LLM replies never become cached empty successes.
- **Dependency advisories (release day)** — `quick-xml` 0.37/0.39 → 0.41 and `calamine` 0.34 → 0.36 close RUSTSEC-2026-0194/0195 (quadratic attribute-check and unbounded namespace-declaration DoS in XML parsing, fresh in the advisory database when the release CI first ran); office-crate call sites moved to the 0.41 decoder API.
- **Everywhere else** — XLSX decompression bounds are pre-checked via a bounded `<dimension>` zip scan before allocation; non-ASCII document titles can't panic filename creation; watch mode covers every supported file type and directory renames; database decode errors surface instead of silently truncating graphs; Bolt 3 RUN carries its third (extra) field per the official spec; risk traversal propagates errors; the viewer bundle is drift-checked in CI; a version-to-version A/B harness ships in `scripts/bench/ab/`.

### Follow-up review — publication, coverage, health, cache (3–4 October)

- **Cache invalidation is part of graph publication** — the generation advances inside the core transaction and immediately after every derived pass that commits a content change; unchanged runs reuse the previous generation; `cluster-only` republishes through the same artifact workflow as full pipelines.
- **Merge-gate coverage is commit identity, not timestamps** — the pipeline records `_meta.git_head` at publication and `verifySourceCommit` compares it with the current HEAD while re-hashing every manifest file (the graph's own versioned scheme); query headers disclose source drift (modified / deleted / size-changed) separately from graph age, and relative manifest paths resolve against the project root so probes work from any cwd.
- **Health-score heuristics corrected** — containment and co-occurrence edges no longer count as reachability (dead-code detection finds real candidates again); hubs must clear max(10, the graph's own 95th-percentile usage degree), and test-file hubs are reported, not scored.
- **Multi-project snapshot cache** — the process-wide graph cache becomes a bounded LRU keyed by database path + generation (default 3 entries, `ASTRIA_SNAPSHOT_CACHE_ENTRIES` 1–16); alternating MCP projects share snapshots instead of evicting each other.
- **Retrieval evidence** — the paired runner's report emits exact-symbol ranking (definition recall@5 + MRR) beside file recall and delivered tokens, plus a per-case definition-miss triage list; known failure modes are ranked as evaluation priorities.
- **Documentation** — architecture claims derive from the registry (language counts drift-checked by `scripts/check-docs-sync.mjs`), the snapshot-cache description matches the LRU, and the review backlog carries per-finding status tables.



### Platform & integration — team serving, cloud backends, CI artifacts (#B1–B5)

- **MCP over HTTP + multi-project serving** — `astria mcp --http` serves one or many project graphs over MCP Streamable HTTP (JSON responses; `GET /healthz` for liveness) from a single process, complementing the stdio transport. Clients select a project with the `x-astria-project` header or `?project=` query (unknown names 404 — no silent fallback); `--graph` is the default project and `--projects name=path` adds more. Bearer auth (`--token` / `ASTRIA_MCP_TOKEN`) is mandatory whenever the server binds a non-loopback host — unauthenticated remote serving is refused at startup. No async runtime: thread-per-connection, a fresh SQLite handle per request. Transport routing, auth, and project resolution are tested without sockets.
- **First-class Azure OpenAI, AWS Bedrock, and Kimi backends** — `--backend azure` authenticates with Azure's `api-key` header against `{endpoint}/openai/deployments/{deployment}` (+ mandatory `api-version` query), `--backend bedrock` calls the Bedrock Converse API with real SigV4 request signing (self-contained HMAC-SHA256 implementation pinned to independently computed reference signatures; static keys and `AWS_SESSION_TOKEN` temporary credentials both work), and `--backend kimi` is the OpenAI-compatible surface pointed at Moonshot with Kimi's key variables and default model. Bedrock's Converse usage format (`inputTokens`/`outputTokens`) joins the usage counter's understood wire formats. All three are documented in env-vars.md with their ASTRIA_* and vendor env names.
- **Rich PR dashboard** — `astria prs` now pulls CI state (`statusCheckRollup`), review decision, mergeability, author, and diff size in one `gh pr list` call, maps PR branches onto `git worktree list` locations, ranks the review queue by urgency (failing CI, requested changes, conflicts, graph blast radius, draft penalty), and prints merge-order risk on `--conflicts`. `--triage` gives compact per-PR lines; `--json` emits the ranked queue with every signal.
- **Git merge driver for the graph file** — `astria merge-driver install` wires a three-way union-merge driver into `.gitattributes` + `merge.astria.*` git config so parallel branches that both commit `.astria/graph.json` merge instead of conflicting: additions from both sides survive, deletions are respected, fields resolve 3-way (unchanged side takes the changed side), and communities (derived data) resolve to whichever side moved. `.astria/graph_report.md` gets git's built-in `union` driver. `uninstall` removes the wiring; `run` is the git-invoked entry point.
- **Docker distribution** — a multi-stage Dockerfile in the repo root (Rust+Node builder → slim Node runtime) ships the full CLI with no toolchain inside; analyze a mounted repo or serve the HTTP MCP on exposed port 8620; `--build-arg NAPI_FEATURES=--no-default-features` produces a smaller image without the embedding runtime.
- **Hosted-tier OSS surface** — `astria merge-gate` is a CI check that fails on missing/stale graphs (publish timestamp vs wall clock and last commit), health-score floors, and diff blast-radius ceilings (`--json` for pipelines); `astria digest` renders a deterministic markdown engineering brief (overview, health, hub concentration, largest communities, LLM spend) for stdout, `--out`, or cron. These are the same primitives the hosted tier (app.graphify.com) operates for teams.
- **Deep-clean uninstall** — `astria uninstall --purge` removes every platform install plus the artifacts plain uninstall deliberately leaves: git hooks, merge-driver wiring, the project `.astria/` data directory, and the `~/.astria` global store. Explicit-flag consent, no prompt, CI-safe.

### Query & graph features (#C1, #C3, #C4)

- **CJK query segmentation** — the retrieval tokenizer now segments Chinese/Japanese/Korean runs with jieba (dictionary + HMM, built once per process), so "用户登录怎么处理" matches the labels that say 用户 and 登录 instead of arriving as one unmatchable character run. Non-CJK tokenization is byte-for-byte unchanged; `nearest_labels` suggestions now share the tokenizer (with stopword filtering), so did-you-mean works for CJK too.
- **Clustering controls** — `astria cluster-only --resolution <0.0–1.0>` requires a minimum share of a node's neighbors to agree on the winning community before the node joins it (default 0.0 = classic propagation; higher values → more, smaller communities, same direction as Louvain's resolution), and `--exclude-hubs` holds high-degree hub nodes (degree ≥ max(12, 4× mean)) out of label propagation entirely so they cannot glue communities together — hubs are attached to their strongest community afterwards, so every node still lands in one. Defaults reproduce the previous behavior exactly; hub exclusion also pre-seeds the fragment-merge pass so hubs are never absorbed as fragments.
- **Cost report artifact** — every `run`/`update` writes `.astria/cost.json`: this run's measured LLM spend (from the pipeline_runs row), lifetime totals across completed runs, backend/model identity, and — when `ASTRIA_COST_INPUT_PER_MTOK`/`ASTRIA_COST_OUTPUT_PER_MTOK` are set — a dollar estimate clearly labeled as operator-supplied rates, not vendor billing. Best-effort write: a report failure warns and never fails the build.

### Ingestion & language parity expansion (#A1–A6)

- **17 new registered languages** (25 → 42): SQL (tables/views/functions + `CREATE TRIGGER` via `tree-sitter-sequel`), Julia, R, Fortran, Solidity, Groovy, Luau, Objective-C, OCaml + OCaml Interface, Common Lisp, BYOND DreamMaker (`.dm`), Astro (`.astro`); Vue + Svelte extract embedded `<script>` TS/JS via the JS/TS grammars (`langs::embedded`); VB.NET + Pascal/Delphi use regex declaration extraction (no upstream grammar crate). Every language ships its own extraction test.
- **Office documents**: new `astria-office` crate — `.docx` (headings/lists/tables via `word/document.xml`) and `.xlsx` (per-sheet markdown tables, 500×30 cap) become document nodes; classified as `Document`.
- **Google Workspace**: new `astria-gws` crate — `.gdoc`/`.gsheet`/`.gslides` shortcuts resolve a Drive file id and export via Drive API v3 (Docs→text, Sheets→CSV→markdown table, Slides→text); auth via `ASTRIA_GDRIVE_ACCESS_TOKEN` or gcloud ADC refresh; missing credentials skip with a notice, never fail the build (whisper-route semantics, results uncached).
- **Media URLs**: `astria add` classifies YouTube/Vimeo/Dailymotion/Twitch links and direct media URLs as `UrlKind::Media` and downloads via external `yt-dlp` (16 kHz mono WAV with ffmpeg, raw bestaudio without) into the project for whisper transcription; yt-dlp missing degrades to a stub node with the install hint.
- **Doc formats**: `.qmd` rides the markdown path, `.html`/`.htm` are tag-stripped (scripts/styles dropped, entities decoded), `.yaml`/`.yml` chunk as text — all classified `Document`.
- **Rationale comments** generalize across comment styles (`#`, `--`, `;`, `'`, `!`) — hash-comment languages (Ruby, Shell, Elixir, Lua, and the new Julia/R/Groovy/SQL/Luau/Common Lisp/VB.NET/Fortran) now emit `rationale` nodes.
- Grammars are compile-time optional as before (new `lang-*` features incl. `lang-ocaml-interface`); `EXTRACTION_HASH_VERSION` unchanged — none of these types were previously ingested, so no cache invalidation is needed.

### Video/audio ingestion — Whisper transcription (#82)
- **Media files join the graph** — `mp4`/`mov`/`webm`/`mkv`/`avi` video and `mp3`/`wav`/`m4a`/`flac`/`ogg`/`opus`/`aac`/`wma` audio files are transcribed during `run`/`update` and enter the graph as transcript documents through the same markdown pipeline PDFs use. External-binary mode, like `add --postgres` requiring `psql`: transcription runs in [whisper.cpp](https://github.com/ggml-org/whisper.cpp)'s `whisper-cli`, video files also need `ffmpeg` on PATH to demux the audio track (audio-only repos transcribe without it). Nothing is vendored, no API key is involved, and the napi binaries stay small.
- **Missing tooling degrades to a notice, not a failure** — without `whisper-cli`, a model, or (for video) `ffmpeg`, media files are skipped with one actionable notice per cause per run while the rest of the graph builds normally; failed attempts are never cached, so installing the tooling is picked up on the next run even for unchanged files. Model resolution order: `$ASTRIA_WHISPER_MODEL`, then the first `*.bin` in `<project>/.astria/models/`, then `~/.astria/models/`.
- **Plumbing** — `FileType` gains `audio` (detect classifies the new extensions; the extraction cache hash bumps to v13, forcing one clean re-extraction on upgrade), and a std-only `astria-audio` crate joins the workspace between `astria-ingest` and `astria-pdf`.

### P0 hardening — ignore-file failures surface, embed capability is disclosed, one budget default
- **A `.astriaignore` that cannot be loaded now fails the run.** File discovery previously swallowed `add_ignore` errors, so an unreadable or misplaced `.astriaignore` silently built the graph without the user's exclusions. Malformed individual patterns still follow gitignore's lenient semantics (an unclosed `[` is a literal, matching git's own behavior). Regression-tested for both the exclusion behavior and the loud failure.
- **Embed capability is disclosed, not discovered.** `graph_stats` (CLI `--json` and the MCP tool) now reports `embeddingsSupported` / an `embeddings: available | not supported in this build` suffix, and `stats` prints the same in human output, so a darwin-x64 user (no prebuilt ONNX binaries) learns the `--embed` gap before hitting the runtime error. Documented in the README quick start.
- **One default query budget.** The CLI (`query`, `map`, and the injected `astria_query`/`astria_map` tool presets) and the MCP server now share a single 2,000-token default (`DEFAULT_QUERY_BUDGET` in the MCP server, `src/defaults.ts` in the CLI, guarded by a structure test); the injected `astria_query` preset previously fell back to 3,000 while everywhere else used 2,000.

### P1 hardening — module splits, feature-gated grammars, CI gates, integration tests
- **astria-query and astria-semantic split into domain modules** — the 4,300-line query god file becomes store/scoring/render modules behind an unchanged public API; the semantic crate separates shared types/prompt/chunking/http from one module per backend (Claude, OpenAI-compatible, Gemini, Jev). Pure extraction, zero behavior change; every test passes untouched.
- **Language walker branches move into `langs/`** — Rust doc-comment/test-attribute handling, Python overload semantics, the JavaScript name/binding/doc walker, and PHP route-label synthesis now live beside their language configs; `walkers.rs` keeps only language-neutral machinery.
- **Tree-sitter grammars are feature-gated** — each language gets a `lang-*` cargo feature (`lang-all` remains the default), and the engine skips files of languages compiled out with a one-time warning instead of failing. `--no-default-features --features "lang-python,lang-javascript"` now produces a slim extraction build. The three ad-hoc grammar pins move into `[workspace.dependencies]`.
- **astria-napi stops being a monolith** — the seven export formats move to a new `astria-export` crate, the health/risk diagnostics join `astria-analyze`, and the Neo4j push joins `astria-bolt`. All moved code was napi-free; `astria_napi::export_wiki::…` paths re-export unchanged.
- **CI gates** — a new MSRV workflow compiles the workspace on the declared Rust 1.88 floor; a Coverage workflow publishes per-crate llvm-cov numbers (informational until a baseline exists); CONTRIBUTING documents the versioning policy (features minor, fixes patch — no `cargo-semver-checks` because no crate is published).
- **Integration tests** — the MCP stdio loop is now a transport-independent `serve_loop` with framing tests (response-per-line, malformed-line skip, stop-at-EOF), and `astria-bolt` gains `#[ignore]`d live-Neo4j tests (handshake, RUN/PULL round trip, auth-failure-is-an-error) runnable against Docker.

## [1.0.11] — 2026-10-02

### Code audit — data safety, installer ownership, native loading, CI and site
- **DB migrations are atomic and self-healing** — each schema step now commits its DDL and its `schema_version` bump in one transaction (SQLite DDL is transactional), and ALTER steps check `PRAGMA table_info` first. Before, a crash between an ALTER and its version stamp left a database whose next open re-ran the ALTER, failed with `duplicate column name`, and bricked every later command against that repo; the idempotent guard also repairs databases the old code had already stranded. Regression-tested (`interrupted_migration_is_repaired_not_fatal`).
- **The installer no longer wipes unparseable configs** — `readJson` used to swallow any JSON parse error and return `{}`, which the subsequent rewrite made permanent. A non-empty unparseable file now aborts the install with a refusal naming the path; a UTF-8 BOM is tolerated; and `.vscode/mcp.json` (officially JSONC — comments and trailing commas are legal) is parsed with a conservative comment/trailing-comma stripper so commented team configs install without losing their servers. All installer config rewrites (JSON, the Codex TOML, markdown sections, git hooks) now write via temp-file + rename, so a crash mid-write can never leave a truncated file — which is what previously turned into a wipe on the next install.
- **Uninstall removes only what install wrote** — three ownership gaps closed: `removeAgentMcp` deleted any `astria`/`graphify` MCP entry regardless of authorship (a user-written entry with those names survived install's own preserve check only to be deleted on uninstall); `removeSection` deleted unmanaged `## astria` markdown sections that install correctly treats as user-owned, and its heading match was prefix-based, so `## astria-guide` was caught too; and hook entries were claimed by bare substring — a user's own `astria hook-guard read --strict` PreToolUse hook (the command our own docs suggest) was deleted by `astria uninstall claude`. MCP ownership now uses one `isInstallerServer` predicate across install/uninstall/legacy cleanup; markdown removal requires the managed marker; hook matching keys on the structural fingerprints every installer template carries (a quoted `.astria`/`.graphify` path segment, or the pre-1.0 package name).
- **The updater hook is no longer appended to shell hooks** — appending the JavaScript updater to a `#!/bin/sh` hook (a user hook, or a husky-style hook under `core.hooksPath`) broke that hook with syntax errors on every commit while the graph refresh silently never ran; the old test asserted the appended file content but never executed the hook. Install now appends only to Node-script hooks and skips others with an explicit notice (a foreign Node hook still merges cleanly — both paths tested).
- **A missing native binary no longer kills the whole CLI** — the platform-package `require` had no try/catch (the crafted diagnostic was unreachable in exactly its target scenario), the musl switch arms require packages that were never published (Alpine was a guaranteed raw `MODULE_NOT_FOUND`), and the binding loaded at module scope, so even `astria install`, `astria uninstall`, and `--version` crashed before Commander ran. Every require is guarded, the binding loads lazily on first native call, and the diagnostic names the resolved platform target and the musl limitation.
- **Bolt decoding bounds server-controlled sizes** — PackStream list/struct lengths from a Neo4j server fed `Vec::with_capacity` unbounded (a hostile LIST_32 length was an allocator abort, uncatchable across the napi boundary), and message deframing had no total-size cap; both are now bounded (element count clamped to remaining bytes, messages capped at 256 MiB with an `InvalidData` error). Hostile-input tests added for all three decoders and the frame cap.
- **Copilot's skill file installs where Copilot reads it** — repo-scoped `.github/skills/` in the project (docs.github.com), not `~/.github/skills/`; the test's own comment already said project-scoped while asserting the home path. Installs clean up the 1.0.9/1.0.10-era home-dir copy, and file-stem layouts (cline/roo) keep their identity guard so a second install still never deletes its own skill.
- **Smaller correctness fixes** — unknown `--platform` now exits 1 (it printed `Unknown platform` and exited 0, so scripts could not detect the failure); uninstall reads `CLAUDE_CONFIG_DIR` through the same sanitizer install uses and removes both candidate skill roots (the raw env var previously reached an `unlinkSync` unvalidated); `npm run napi:dev -- --debug` builds no longer lose to a stale release artifact (candidate order follows the profile just built); bench-snapshot's two pushing jobs are serialized (`needs:` — the concurrency group serializes runs, not jobs within a run, so one push used to lose the race) with permissions narrowed per job; `promptfoo` is pinned (`@0.123.1`) instead of `npx promptfoo@latest` with the judge API key in env.
- **Website truthiness** — homepage language count corrected to 25 (the registry has 25; README already said 25); the 1.0.10 docs version cut and `lastVersion` refresh (default `/docs` was five releases stale at 1.0.5); release-notes blog posts for 1.0.9 and 1.0.10, which the sidebar's "Release notes" feed stopped at 1.0.8.

### Distribution audit — four fixes
- **Homebrew formula installed no executable** — `std_npm_args` installs global-style into `libexec` (package at `libexec/lib/node_modules`, executables linked at `libexec/bin`), but the 1.0.10 formula symlinked `libexec/node_modules/.bin/astria` — a local-install path that never exists under `libexec` — and Homebrew's `install_symlink` over an empty glob is a silent no-op, so `brew install nodesify/tap/astria` "succeeded" with no `astria` command. The formula now symlinks `libexec/bin/*` (homebrew-core's idiom for npm packages) at `revision 1`; the live tap carries the same fix.
- **The Claude Code plugin shipped without its MCP server** — the plugin root is the repo root (marketplace `source: "./"`), and its only MCP registration was the root `.mcp.json`, which is machine-local and gitignored — so marketplace installs delivered the skill, commands, and subagent but zero MCP servers, despite 1.0.9's "the tracked `.mcp.json`" changelog claim and the plugin's own description. `.claude-plugin/plugin.json` now declares the `astria` stdio server inline via `mcpServers` (the same `astria mcp` entry `astria install` writes), which ships with the tracked tree. Plugin and marketplace metadata move to `1.0.11` ahead of the npm package so version-caching plugin managers register the changed plugin — the npm package, `server.json`, and the registry listing stay at 1.0.10 until the next tagged release.
- **The release verify step checked versions only** — the field that actually broke v1.0.9 (`mcpName` vs `server.json` name, the 403 namespace case mismatch) was never compared, so a future drift would again surface only at the post-npm registry step where immutability makes it unfixable without burning a version. The step now verifies name vs mcpName, the npm entry's identifier vs the package name, and the per-package version, alongside the top-level version.
- **mcp-publisher is pinned and checksum-verified** — the registry publish step downloaded `releases/latest` and executed it with the job's OIDC and GitHub tokens, the only unpinned external code in a workflow where every action is SHA-pinned. Now pinned to `v1.8.1` with a sha256 check; moving to a newer publisher is a deliberate tag+checksum bump.

## [1.0.10] — 2026-10-01

### Distribution fix — MCP Registry namespace case
- **v1.0.9 reached npm but not the registry** — the official MCP Registry publish failed with 403: the GitHub OIDC grant is `io.github.Nodesify/*` (the org login's case is significant) while `server.json` declared `io.github.nodesify/astria`. npm is immutable, and registry validation compares the published package's `mcpName` against the server name exactly — so both move to `io.github.Nodesify/astria` in this release; 1.0.9's lowercase `mcpName` can never validate. Automated registry publishing, the Claude Code plugin marketplace, the Homebrew tap, and `smithery.yaml` are unchanged from 1.0.9.

## [1.0.9] — 2026-10-01

### Distribution — official MCP Registry, Claude Code plugin marketplace, Homebrew tap, Smithery
- **Official MCP Registry publishing is automated** — the repo now carries a registry `server.json` (`io.github.nodesify/astria`, stdio transport over the npm package) and `release.yml` publishes it via `mcp-publisher` with GitHub OIDC after the npm publishes succeed; the `verify` job fails fast when `server.json`'s version drifts from the package version. npm packages must declare the matching `mcpName` for registry validation — added to `packages/astria-cli/package.json`. The first listing goes live on the next tagged release.
- **The repository is a Claude Code plugin marketplace** — `/plugin marketplace add Nodesify/astria` then `/plugin install astria@nodesify` installs, in one plugin: the MCP server (the tracked `.mcp.json`), the graph-first skill (`skills/astria/`), two slash commands (`/astria` graph queries, `/astria-risk` PR-ready risk report), and an `astria-architect` subagent — wired through `.claude-plugin/marketplace.json` + `.claude-plugin/plugin.json`.
- **Homebrew tap** — `brew install nodesify/tap/astria` installs the published npm package; the formula ships in the new [`Nodesify/homebrew-tap`](https://github.com/Nodesify/homebrew-tap) repo, with per-release update instructions in `packaging/homebrew/README.md`.
- **Smithery registry config** — `smithery.yaml` (stdio start command over the published npm package, optional `projectPath`) so smithery.ai lists the server once the repo is connected there.
- **Discovery metadata** — GitHub topics gained `claude-code` and `agent-skills` alongside the existing `mcp-server`/`model-context-protocol` set.

## [1.0.8] — 2026-10-01

### Agent experience — staleness disclosure and MCP tools that teach their use
- **Queries disclose a stale graph** — the header now reports how many manifest files changed since the build (`# 3 file(s) changed since this build — run astria update before trusting answers`), next to the existing `# graph built at` line. Edits through paths without hooks (Claude Code print-mode sessions — which provably do not run PostToolUse hooks — editors without them, plain typing) previously left agents confidently answering from the past unless they noticed a raw timestamp. The check is stat-only against the file manifest (no re-hashing): milliseconds per query even on large repos, and it clears itself on the next `update`. Verified live: touch a file → disclosure appears; `update` → fresh timestamp, silent header.
- **MCP tool descriptions now teach usage** — the descriptions agents see when choosing tools: `query_graph` explains the `file:line` anchors, the provenance tiers, what "No confident match" means (rephrase toward symbol names — not an error), and that the header discloses staleness; `explain` documents the `-->`/`<--` real-direction arrows; `affected` says to run it *before* changing a shared symbol and how to read RESOLVED vs INFERRED hops. Verified through a live MCP handshake that the served descriptions carry the guidance.

### Full ecosystem coverage — MCP for every coding tool, five new platforms, `install --all`
- **MCP registration everywhere it is supported** — joining claude/cursor/gemini/zcode: **VS Code** (`.vscode/mcp.json`, native workspace MCP — covers every VS Code-based editor including Copilot inside it), **Codex** (`~/.codex/config.toml`, user-global TOML — the installer appends a managed `[mcp_servers.astria]` table and never touches a hand-written one), **Trae**, **Windsurf**, **Kiro** (`.kiro/settings/mcp.json` — Kiro's documented workspace-scope config), and **OpenCode** (`.opencode/opencode.json`). The **Copilot coding agent** gets skill + instructions only: it has no committed repo MCP file — repository-level MCP is JSON pasted into the repository Settings on github.com. All JSON flavors are project-scoped, idempotent, and merge-safe like the originals; `uninstall` removes each, and configs an earlier build wrote at a since-corrected path are migrated away, never duplicated.
- **Verified against the real vendor CLIs and vendor docs, not just schemas**: `codex mcp list` shows `astria … enabled` (Codex 0.140.0 parsing the appended TOML); `gemini mcp list` resolves the settings entry; `opencode mcp list` reports `✓ astria connected` — it actually launched the server; `claude mcp list` reaches the project `.mcp.json` server (⏸ pending Claude Code's documented one-time project approval). Real-tool testing caught two OpenCode bugs schema-following would have shipped: opencode 1.17+ **rejects** a `plugins` key in `opencode.json` (the pre-existing plugin injector wrote one — plugins now drop into the auto-discovered `.opencode/plugins/` — the convention in opencode's current docs, and verified loadable by probing `opencode debug config` on 1.17.8, which resolves plugins from both the plural and singular directories — with no config key, and installs upgrade away both earlier layouts), and its MCP schema requires servers **directly under `mcp`** with `{type: "local", command: [...], enabled: true}` — not `mcp.servers` with a `stdio` shape. It caught one in ours too: VS Code 1.137's own `--add-mcp` writer uses a bare `servers` map (no `mcpServers` key, no `type` field), so the vscode flavor now matches the vendor's writer exactly, migrating 1.0.8-era entries. User-customized entries are never touched in any flavor. The exact commands every config runs were also proven end-to-end: an MCP stdio handshake against `astria mcp` (initialize → 10 tools → real `query_graph`/`affected` calls).
- **Three install-path corrections from a docs audit of this repo's own agent integration** (all caught before release, against vendor documentation): **Kiro**'s workspace MCP config is `.kiro/settings/mcp.json` per kiro.dev's configuration docs — not a bare root `mcp.json`, which no Kiro version reads (the dead file a dev build had written is deleted on install/uninstall when it only carries our entry); **OpenCode**'s documented project plugin directory is `.opencode/plugins/` (plural) — the singular `plugin/` still loads on 1.17.8 but is not the documented convention, so the installer writes the plural directory and migrates both legacy layouts; and the **Copilot coding agent** reads repository-level MCP only from repository Settings on github.com (confirmed in github/docs: "Configure MCP servers for your repository" — JSON entered in the Settings UI, `mcpServers` shape) — the `.github/copilot-mcp.json` a dev build wrote is read by nothing and is cleaned up. The injected `## astria` instruction block also caught up with the tool surface: it now names all ten MCP tools (it still said six, omitting `god_nodes`, `list_communities`, `graph_stats`, `health`), and this repo's tracked `AGENTS.md` is now itself a managed section (`<!-- astria:managed -->`) so `astria install` keeps it in sync instead of treating it as user-owned forever. The repository's own tracked dead copies (root `mcp.json`, `.github/copilot-mcp.json`) are dropped in the same change — every working per-tool config stays machine-local behind `astria install`, with `AGENTS.md` and `.github/copilot-instructions.md` (GitHub's documented repo instructions file) the only tracked agent artifacts.
- **Six new platforms**: `vscode` and `windsurf` (MCP-only — no skill-file mechanism to target), `cline` (skill → `~/.clinerules/`, AGENTS.md), `roo` (skill → `~/.roo/rules/`, AGENTS.md), `amp` (AGENTS.md — Amp reads it natively), and `pi` — the Pi coding agent gets an auto-discovered extension (`~/.pi/agent/extensions/astria.mjs`, every API surface verified against pi 0.87's types) that registers the graph as **native pi tools** (`astria_query`, `astria_map`, `astria_explain`, `astria_path`, `astria_affected`, CLI-backed — pi's own philosophy is registered tools over MCP definitions: a few hundred context tokens vs 10k+), plus automatic graph refresh around tool calls and an `/astria` guidance command. Freshness is mode-independent by measurement: pi 0.87 print-mode sessions never deliver `tool_result` events to extensions (an instrumented listener captured zero events — not even for the extension's own tool calls), so every graph tool call itself triggers the throttled refresh, awaited until the update process exists before returning — the session exits the instant a tool returns, and an un-awaited detached child never materializes (both failure shapes measured). Verified end-to-end: the parent exiting immediately after the tool returns does not kill the update — `graph_published_at` advances. The standard `.mcp.json` stays registered for `pi-mcp-adapter` users at zero extra cost — the adapter reads it natively and was observed discovering the astria server. Fully standalone — no adapter required: verified in a pristine pi (`--no-extensions -e astria.mjs`, every other extension including pi-mcp-adapter disabled) where the extension loads and registers cleanly; pi's own provider serialization consumes plain JSON-schema parameters, which is exactly what the extension registers. All five tools executed through pi's AgentToolResult contract returning real graph data (the MODEL const, RESOLVED-tier blast radii, ranked repo map), and a project without `.astria/` gets a build-one-first hint instead of a raw CLI error. The full chain was then verified live with a real pi session (glm-4.7): the model called the native `astria_query` tool on its own and answered from the graph — `jinaai/jina-embeddings-v2-base-code` at `crates/astria-embed/src/lib.rs:13-15`, file:line anchor and all. The platform roster is now sixteen.
- **`astria install --all` / `uninstall --all`** — one run wires every supported platform; a single-platform `install` now prints the remaining platforms so multi-tool users discover the rest. Previously the command silently defaulted to claude-only, and a Codex or Trae user who ran `astria install` got the wrong layout without a hint.
- Tests: the MCP flavor matrix grew from 4 to 9 parametrized JSON flavors (shape, idempotence, preserve-others, removal) plus dedicated OpenCode-shape/migration, Kiro legacy-path migration, Copilot legacy-file cleanup, Codex TOML round-trip (user-config preservation, never-clobber), and file-stem layout tests; install suite 292 assertions green.

### Answer trust and first-rank retrieval — RESOLVED edges, honest directions, Rust docstrings, adaptive semantic ranking
- **`RESOLVED` edge tier for uniquely-bound calls** — a call expression is extracted from source, but its binding (which definition the bare name means) is name inference; the graph previously called the whole edge `INFERRED`, so `affected` marked all 12 depth-1 callers of `score_nodes` as untrusted. Reference resolution now upgrades a call whose name binds to exactly one definition to `RESOLVED` (strength 0.85): above co-occurrence inference, deliberately below `EXTRACTED`/`DECLARED` so `--detail high` (compiler-grade facts) and health's EXTRACTED-only cycle detection still exclude it. On this repo: 3,013 of 11,828 call edges are `RESOLVED`; the remaining 8,815 (`is_some`, `join`, unbindable names) stay honestly `INFERRED`. `affected` shows tiers per hop and saves the legend line for genuinely untrusted hops.
- **`explain`/`neighbors` show real edge direction** — the explained node was rendered as the source of every connection arrow, inverting caller/callee for incoming edges (the very artifact that made `model_cached()` look like it called its callers). Connections now render `-->` (this node calls/imports the neighbor) vs `<--` (the neighbor calls/imports this node), on CLI and MCP; `--json` carries `outgoing`.
- **Rust `///` doc comments and `//!` module docs are extracted** (extraction rules v12) — functions, structs/enums/traits, and file nodes now carry their documentation as docstrings, feeding the doc-evidence ranking, the embedding text, and `explain`. Before v12, Rust had zero extracted docstrings: the ranking layer's docstring evidence and the open-bench audit's "docstring evidence" fix had nothing to work with in Rust code. 405 code nodes on this repo gained docstrings; only text-changed nodes re-embed (320 on the migration `update`).
- **Description-shaped queries let strong embeddings rank** — when none of a question's identifying (salient) terms has lexical evidence anywhere ("auth flow" against a graph that never uses those words), a strong calibrated cosine now maps into the label-match tier (2.0 at `strong_match`, capped 2.6 — still under an exact label match's 4.0) instead of capping below every partial token match and waiting for the reserved seed slot. When the graph does answer the question's vocabulary, the tie-breaking cap stands unchanged. The mapping lives on `astria_core::calibration::SemanticCalibration` (`description_seed_score`), measured anchors like every other threshold.
- **The embedder loads once per process** — the ONNX session (~1s, ~600 MB) is cached behind a lock and reused across queries; the MCP server's steady-state embedded query drops from ~1.4s to ~140ms (measured 917ms first call → 137ms cached in one process), and one instance bounds memory regardless of worker threads.
- **Measured end-to-end** (self-corpus golden, 35 questions, same graphs rebuilt at v12): MRR 0.636 → **0.682**, hit@1 51.4% → **60.0%** (+3 questions answered first), hit@10 unchanged at 91.4%. The previously-missing "what breaks if I change a function blast radius" now surfaces `affected.rs` (the description-shaped semantic path, rank 15 behind the prose that literally documents the feature — prose-first on feature-intent questions is kept deliberately). Two honest non-wins: "which languages does tree-sitter extraction support" still misses (the docs page and generator script genuinely answer it as well as the golden code files — golden ambiguity, not ranking), and the partial-overlap probe ("untrusted paths blocked…") still seeds on the dedup `*_blocked()` functions whose labels the question's common words really do match — that shape needs phrase-level intent matching, not another weight.

### Embedding model swapped for code-aware retrieval
- `astria-embed` now runs **jina-embeddings-v2-base-code** (fastembed/ONNX, 768 dims, 8k-token context, ~615 MB one-time download) instead of bge-small-en-v1.5. The swap targets the measured binding constraint from the open-bench audit: a natural-language description must rank its true function among thousands of code nodes, and the previous general-prose model's reserved semantic seed hit the RepoQA needle 0/10 times on psf/black. Model choice keeps the local-first property (no API key, offline after download); the one-time download grows from ~90 MB to ~615 MB and all docs now say so.
- Embedding reads are **model-scoped**: `rebuild_similarity_edges` and `semantic_scores` only score vectors stored under the current model, so a graph that still carries pre-swap vectors never mixes cosines across models (or dimensions); `embed_missing_nodes` already re-embeds on model mismatch, and `model_cached()` now probes the current model's cache directory specifically instead of accepting any downloaded model — so a query-time load can never start a surprise download when only a different model is cached.
- Node text cap raised 1,500 → 4,000 chars (the jina model's 8k-token context takes full docstrings without harmful truncation), and the ignored real-model round-trip test's absolute floor drops 0.6 → 0.45 for the new model's cosine scale.
- **Cosine thresholds now live in one measured calibration table** (`astria_core::calibration`, `crates/astria-core/src/calibration.rs`) instead of scattered bge-scale constants: noise floor 0.45, strong-match 0.70, reserved-seed slot floor 0.52, `similar_to` threshold 0.75 — all measured on the Click corpus with the `astria-embed` calibrate example (`cargo run --release -p astria-embed --example calibrate`), and `astria-query`'s `semantic_seed_score`/seed-slot floor now derive from it. Inheriting bge's 0.55 anchors would have dropped jina matches below 0.55 before ranking ever saw them (measured: jina's noise p95 sits at 0.24–0.44 and relevant matches at 0.31–0.69 on psf/black needles, while bge's compressed scale put half of all nodes above 0.55). Model-swap separability also improved structurally: only 0.79% of node pairs clear cosine 0.60 under jina vs 23.1% under bge.
- The open-bench and LoCoMo runners accept `ASTRIA_BIN` to measure a working-tree CLI without installing it (`repoqa.mjs`, `run-locomo.mjs`; `locomo-qa.mjs` gained `--astria`).
- **Validation, measured end-to-end** (same 4-repo/40-needle RepoQA sample, current CLI, fresh graphs): structural 40.0/60.0/75.0 file hit@1/5/10 → jina+calibration 40.0/62.5/**80.0** (func hit@1 unchanged at 25.0; psf/black needles identical to structural — its descriptions share vocabulary with the code, so token evidence dominates and the semantic layer only adds at the margin). On prose, LoCoMo retrieval over 300 questions is neutral-to-slightly-positive (recall@1 63.0→63.7, recall@5 85.7→85.3 — noise-level). Honest reading: the swap buys a decisively better semantic substrate (separation and selectivity above) and a modest end-to-end gain at the deep ranks (hit@10 +5 points), not a needle-finding breakthrough; most RepoQA needles are not pure zero-vocabulary queries, which is where the reserved semantic seed matters. Exploiting the better cosines in the merged scoring path (heavier semantic weighting for description-style queries) is the identified follow-up, not another model swap.

### Judge-layer A/B harness
- New `scripts/bench/quality/judge-ab.mjs` productizes the one-off 2026-09-28 modes experiment into a committed harness: build (or reuse, via `--mode-dir`) `plain`/`llm`/`llm-jev` graphs over one corpus, score every golden set at each budget × detail tier with the run-quality file-rank methodology, and emit a results JSON plus markdown report with graph shape, build tails, and engine/judge provenance. `plain` runs keylessly; `llm` needs the engine key; `llm-jev` additionally requires a Typesafe key and the harness fails before any build spend when one is missing. Keyless re-scoring of the prebuilt 2026-09-28 mode graphs reproduced the experiment's conclusion: on Click, frozen/heldout-v1 sets are perfect on every mode, llm-jev costs a little heldout-v2 recall@5 (80% vs 100%) and doc-intent is mode-neutral — the judge changes graph shape (337 vs 153 communities; 201 fewer edges), not retrieval on these sets. The harness records the judge's measured gate stochasticity (31 vs 17 files gated on identical replay) in every report.

### LOCOMO QA decomposed — retrieval vs reader vs ceiling, shared-model Graphify leg
- `scripts/bench/open/locomo-qa.mjs` now decomposes QA accuracy into three legs per tool: **retrieval** (gold evidence sessions in top-1/top-3, keyless), **retrieved** (the published reader-over-top-3 leg), and a tool-independent **ceiling** (same reader and assembly over the gold sessions). The summary splits every retrieved-leg failure into retrieval miss vs gold-retrieved-but-lost (and the latter into assembly-dilution vs reader-limit), with per-category accuracy. `--tool astria|graphify|both` runs the shared-model protocol — both tools' retrieved legs under one reader and judge in a single process, the cross-publishable shape.
- First decomposed run (n=30 non-adversarial, gpt-4o-mini reader+judge, fresh 1.0.7 graphs): astria retrieved-leg accuracy **33.3%** (mean coverage 0.325) against a **56.7% gold-context ceiling** — retrieval gold@3 66.7%, gold@1 53.3%. Of 20 failures: 9 were retrieval misses, 11 had gold in the top-3 (6 of those the reader answered from gold alone — context assembly dilution; 5 the reader lost even with gold — reader/judge limit). So roughly a third of the remaining gap is retrieval, a third assembly, a third reader/protocol — and the 12k-char context cap is a stated protocol bound. The same 30 questions scored 23.3% under the pre-1.0.7 pinned CLI, so the prose-ranking fixes carried ~10 points end-to-end.
- **Graphify structural on prose is a null result, now measured**: `orig_run.py --include-documents` feeds detected markdown through graphify's own `extract_markdown` dispatch (opt-in; code-corpus benchmarks unchanged), yet its label-seeded query still surfaces nothing for prose questions — 0% gold@3 (1/30), 0% QA accuracy under the same reader and judge, 29/30 failures retrieval misses. Graphify's published LoCoMo numbers therefore rest on its LLM layer; its structural substrate cannot rank prose content.
- The QA jsonl evidence names are matched to ranked paths by basename (evidence entries are bare session names; ranked paths carry the `.astria/transcripts/` prefix).

### First measured comparison against targeted search
- The external runner's deterministic baseline (question-derived `rg` terms, term-occurrence file ranking, source windows around first matches) now has a checked-in measurement ([`worked/external-baseline/`](worked/external-baseline/)). Across the eight Click/Express/ripgrep golden questions, structural astria retrieval ranked the defining file first for 8/8 at every budget from 250 to 4,000 tokens — 184–234 delivered tokens per response at the 250 budget, zero clipping — while the baseline never ranked it first at any budget (0/8 in the top five at 250–500, 2/8 at 4,000, best ranks 4–12). Relative to this single-pass floor that is ~19× less delivered context for strictly better first-shot ranking. Boundaries carried in the write-up: deterministic baseline, not an expert iterative searcher; n=8 symbol-heavy questions; one observation per condition; graph-build cost excluded from token accounting. The docs site's [retrieval validation page](website/docs/explanation/retrieval-validation.md) gains the September 30 section.

### Answer trustworthiness — provenance, no-confident-match, model-swap self-heal, constant nodes
- **`affected` labels INFERRED hops** — blast-radius hits now carry the traversed edge's provenance and render it (`spec() (calls INFERRED) via tests/spec.rs`, with a legend line on CLI and MCP; `--json` includes the field). Measured motivation: INFERRED call edges are reconstructed from name references and their direction is not guaranteed — three real call sites call `model_cached()` while the graph recorded the reverse — so a radius that mixed them silently with source-verified edges overstated certainty exactly where agents are told to trust it.
- **File cycles are EXTRACTED-only** — `astria health`'s file-cycle SCC now consumes only edges the source literally contains. Including INFERRED `calls` edges previously chained 82 unrelated files (astria-pdf, viewer.js, bench scripts) into one "cycle" — 10 of the 26 deducted health points rested on that single artifact.
- **No-confident-match floor** — when no node in the graph matches any of the query's salient (highest-IDF, answer-identifying) terms AND no node matched even half of the effective terms, `query` returns an explicit miss naming the missing vocabulary instead of traversing ("No confident match: none of the question's key terms (payroll, overtime, pay) appear in the graph's labels, docstrings, or ids…"). Two measured cases now refuse: "Where is OAuth authentication handled?" (previously seeded `handle_message()` through the fuzzy "handled"→"handle" rescue; 445 nodes of unrelated code) and "How does the payroll module calculate overtime pay?" (previously seeded `index.module.css` on the common word "module" — one full label match out of five terms is real evidence, but it does not identify an answer). Weak-but-on-topic matches are never refused: any node covering half the effective terms ("session handling" → `SessionManager`) traverses, entry-intent queries are exempt (their terms match nothing by design and the import DAG answers them), a qualifying semantic-only candidate overrides (zero-overlap conceptual queries are embeddings' job), and `ASTRIA_QUERY_SEED_FLOOR=off` restores always-traverse.
- **Model swaps self-heal on `update`** — the silent embedding refresh now treats foreign-model vectors as "embeddings exist": an ordinary `astria update` re-embeds them through `embed_missing_nodes`' model-scoped query (previously the current-model-scoped `has_embeddings` gate made the jina swap silently skip, stranding the graph with vectors no query could score). When the new model is not yet cached the refresh still stays offline, but the mismatch is printed with the one-time `--embed` migration command instead of silence (`stored_embedding_models` powers both paths).
- **Rust constants are nodes** (extraction rules v11) — `pub` and `///`-documented `const`/`static` items extract as `constant` nodes whose signature carries the initializer verbatim (`pub const MODEL: EmbeddingModel = EmbeddingModel::JinaEmbeddingsV2BaseCode;`) with the `///` block as docstring; initializer calls attribute to the constant. Value questions ("which embedding model", "what threshold") were structurally unanswerable before — the answer's carrier was not a node. Private undocumented constants stay out. The v11 hash bump re-extracts structurally on the first `update`; the LLM semantic cache is keyed separately and is not invalidated.
- **Release validation** (self-corpus golden, 35 questions, local 1.0.8 build, fresh graph with jina embeddings): MRR 0.636, hit@1 51.4%, hit@5 85.7% (recall@5 81.4%, gate ≥50% passed), hit@10 91.4%, zero failed queries, ~1.4 s per query; token benchmark unchanged (160.5× reduction, ~3,405 tokens per answer). The floor refused **zero** golden questions — both misses ("tree-sitter language support", "blast radius") delivered full ~4,000-token traversals whose prose matches (docs pages) outranked the golden code files, the same rank-drift shape as before. The two refusal probes above were re-measured after embedding the graph: the payroll/helicopter family now traverses when jina's calibrated cosine clears the reserved-seed floor on its on-topic half (ingestion), which is the semantic override working as designed; with embeddings off they refuse. Live probes: `MODEL` is the top-1 seed for "which embedding model" (previously unanswerable), the 82-file health "cycle" is gone (one credible 5-file cycle remains), and `affected score_nodes` marks all 12 depth-1 hops INFERRED — this codebase's function-level call edges are largely inferred, now visibly so.

### Entry-intent phrase matching, explicit corpus mode, transcript writer
- **Entry-point intent matches intact phrases** — the trigger is now a contiguous, stem-equal token run instead of a substring test, and bare `bootstrap` is removed from the phrase list. Two false-positive classes die with it: "domain files" *contains* the substring "main file", and any question touching it re-ranked the import root above every lexical match the question had earned (the boost is a lexical-ceiling override, so a false intent is not a nudge — it wins outright); "bootstrap" names a CSS framework in plenty of repos. "entry points"/"entrypoints" still match via plural stemming; the entry-point golden ("what is the CLI entry point") is unaffected. Regression tests pin both failure shapes.
- **Corpus mode is explicit** — the hidden `prose_share > 0.95` switch that re-ranked chunks/documents and opened the doc-seed quota is now a named, designed threshold (`DOCS_MAJORITY_PROSE_SHARE`) behind an explicit `CorpusMode`: auto-detected by default, pinnable with `ASTRIA_CORPUS_MODE=docs|code` (unrecognized values warn and fall back to auto). The query header discloses the active mode whenever it is not auto-detected code-majority (`# corpus: docs-majority (99% prose)`, or `, pinned via ASTRIA_CORPUS_MODE`), and the no-confident-match floor reads the same mode — one decision, stated in the answer, instead of three scattered comparisons against a magic number.
- **`astria add --transcript <file|->`** — the transcript-sidecar contract gets a built-in writer: a file path is saved under `.astria/transcripts/` with its (filesystem-sanitized) name kept, and `-` reads piped stdin into a timestamped file, so any transcription tool can stream straight in (`whisper ... | astria add --transcript -`). The graph updates in the same run. Before this, the only writer of transcript sidecars anywhere was the LoCoMo bench preparation script.

## [1.0.7] — 2026-09-28

### Jev judge layer — calibrated second opinion over any backend
- New `--judge jev` flag (run/update) layers TypeSafe's Jev — a System One decision model that returns typed judgments with calibrated probabilities, not generated text — on top of the selected `--backend` (claude, openai-compatible, or gemini). The engine still generates every extraction; the judge re-judges it. `--backend jev` is not accepted and errors with a pointer to `--judge` (`ASTRIA_LLM_JUDGE=jev` selects it via env).
- **Trivial-file gate** (on by default, bounded batch sizes) — before a file's first extraction, batched keep/drop judgments (≈1 billed request per 50 files, files >64 KB presumed rich) skip files the judge finds empty or trivial, so they never cost an engine call. Gated files keep their structural extraction; the run summary reports them ("N files gated by Jev").
- **Per-file verification** — one request per file re-chooses node types and relations from the schema allowlists (replacing the lossy `relates_to`/`concept` clamps) and gets a keep/drop existence verdict per edge. Spurious edges are dropped (`ASTRIA_LLM_JEV_MIN_EDGE_PROBABILITY`, default 0.40); kept edges carry the judge's keep probability as a calibrated `confidence_score` in the `edges` table — semantic edges previously left it null.
- **Suggested-question ranking** — on runs that rebuilt the graph, the report's suggested questions are re-ordered by judge keep-scores so the most useful one leads. Best-effort: any judge failure keeps the generated order.
- Judge calls count toward `ASTRIA_LLM_BUDGET` like every other response, and the judge configuration (model, thresholds, gate settings, prompt text) fingerprints into the semantic extraction cache — changing it invalidates cached extractions. `--judge` without `--backend` errors: the judge wraps an engine, it cannot generate extractions.
- Configuration: `ASTRIA_LLM_JUDGE_API_KEY` (or `TYPESAFE_API_KEY`) and `ASTRIA_LLM_JUDGE_MODEL` (default `jev-latest`) are vendor-generic; behavior knobs keep the honest `ASTRIA_LLM_JEV_*` names (`_VERIFY`, `_MIN_EDGE_PROBABILITY`, `_GATE`, `_GATE_MAX_BYTES`, `_GATE_DROP_THRESHOLD`, `_GATE_BATCH`).

### HTML visualization rewritten around drill-down
- `astria export --format html` now ships a self-contained canvas viewer (no vis-network, no network access required) that opens as community bubbles — one per community, sized by membership, with edge-weighted links between bubbles. Click a bubble to expand it into member nodes, click a member to focus its 1-hop neighborhood, and search to jump straight to any symbol; "All nodes" expands everything with level-of-detail labels.
- The exported layout stays fully precomputed (physics-free), and the viewer draws only what is on screen, so large graphs open and zoom instantly even in sandboxed HTML previewers.
- Viewer source lives in `packages/viewer` (TypeScript, `npm run build`); the minified bundle is embedded at `crates/astria-napi/src/assets/viewer.js`. Community bubbles use themed labels from the `communities` table when `--label-communities` produced them.
- **Relation-aware focus** — the exported edge payload now carries the edge kind (`calls`, `imports`, …): the focus panel lists a selected node's neighbors with their relation, and the highlighted 1-hop edges gain direction arrowheads (direction shown where it matters, not on the hairball).
- **Community search** — search matches community names as well as symbols and files; picking a community expands and centers its bubble.
- **Accessibility floor** — the canvas exposes a `role="img"` label with node/community/edge counts plus a visually hidden summary of the controls, so screen readers get a usable description of the export.
- **Quiet, throttled git hooks** — `astria hook install` now writes v4 hooks that invoke `update . --quiet --if-stale 10`: hook-driven rebuilds print nothing (no progress lines, no token benchmark), and skip entirely when the graph was published less than 10 minutes ago, so a burst of commits rebuilds once instead of per commit. `astria update` gained matching `--quiet` / `--if-stale <minutes>` flags; hooks retry plain `update .` against any CLI version that predates the flags, and still never break a commit.

### Skills and MCP updated for the new features
- The shipped skills (`packages/astria-cli/skills/skill*.md`, full + per-assistant variants) now teach agents the new capabilities: the interactive bubble-viewer export (`export --format html`, `--mode standard|large`, the `tree` view, and `--neo4j-push`/`--redis-push`), the full `update` flag set (`--no-dedup`, `--embed`, `--label-communities`, `--deep`, `--quiet`, `--if-stale`), and the git hooks (`astria hook install|uninstall|status`, `hook-guard`) with their automatic post-commit refresh. Existing installs refresh by re-running `astria install`.
- The MCP server's client instructions now point agents at the hooks (`astria hook install`) for automatic post-edit freshness, and the skill's MCP tool list is corrected to include `health`.

### Retrieval ranking tightened for prose corpora
- Chunk labels are a truncated first line of the chunk's own body; scoring no longer amplifies that prefix at label weight for `chunk` nodes, so a later session whose opening line re-mentions a topic cannot outrank the chunk whose body actually answers the question.
- The IDF pre-pass now counts document bodies as well as labels, so terms that are common in bodies but rare in first lines ("group", "friends" in transcripts) stop acting as near-max discriminators, and rare proper nouns carry the ranking.
- Measured on the full LoCoMo set (1,977 questions, structural, no embeddings): recall@1 63.5% → 66.1%, recall@3 79.3% → 80.7%, MRR 0.717 → 0.736, with recall@5/10 at 85.0%. The 35-question code self-check (quality harness) holds recall@5 at 82.9% with MRR 0.636 → 0.659 (a different series from the paired-runner self numbers below — different harness, pinned graphs).

### Qualified-name retrieval and the first blind answer-correctness run
- Question terms now score against each node's scope-qualified id (`BaseCommand.get_usage` reaches `src_click_core_basecommand::get_usage` through the id even though every same-name symbol shares one bare label), and id tokens join the IDF pre-pass so ubiquitous scope words ("src", "core") cannot act as rare discriminators.
- The seed reservation honors qualified names too: an explicitly named qualified symbol reserves its node a traversal seed instead of losing the slot to a label-tie stranger. Click's additional validation went from 0% to 2/2 exact definitions surfaced, additional ripgrep from 25% to 3/4, and the paired-runner self set's MRR from 0.618 to 0.687 at unchanged file recall; LoCoMo is unchanged.
- Blind answer-correctness judging finally ran (TypeSafe System One judge, `scripts/bench/quality/blind-judge.mjs`): both tools answered the same 35 rubric-grounded questions, graded without tool identity — astria 100% PASS, Graphify 77.1% PASS / 2.9% PARTIAL / 20% FAIL. First generated-answer-correctness measurement in the project (single judge, single run, 35 self-corpus questions — not a statistical claim). The same pairs re-graded by the independent promptfoo/OpenRouter judge (`gpt-4o-mini`) agreed on the ordering at 77.1% vs 65.7% pass.
- The paired runner's budgets are configurable (`budgets` array). A four-point budget-response curve (250/500/1000/2000) shows astria's 250-token answers outscoring Graphify's 2,000-token answers (MRR 0.680 vs 0.531, recall@5 74% vs 69%) while Graphify exceeds each of the two smallest budgets on 48/50 raw responses and astria stays inside budget on all 200 (structural only, one observation per condition).
- Two reserved golden tracks exist, authored from pinned source and unused during development: `click.doc-intent-v1.jsonl` (8 doc-intent cases) and `click.reserved-v1.jsonl` (12 cases, doc- and code-intent, line-exact definitions). First use must be an evaluation run; afterwards they count as exercised.
- Held-out evidence grew: `scripts/bench/paired/*.heldout-v2.jsonl` adds 14 separately authored, line-exact grounded cases (5 Click, 5 Express, 4 ripgrep); current runtime retrieves 5/5, 5/5, 2/4 files and 11/13 v2 definitions in the top five.

### Fixed
- **Tree export hardening** — the symbol-tree hover inspector builds its panel with DOM `textContent` instead of `innerHTML`, and the embedded JSON escapes `</script`/`<!--` breakout sequences: the tree viewer now upholds the bubble viewer's labels-as-text safety property, with matching tests.
- **Docs drift** — reference pages corrected against the code: query-log env semantics (`ASTRIA_QUERY_LOG` is a literal path; `ASTRIA_QUERY_LOG_ENABLE` selects the default), the `--json` 20-neighbor cap, `--detail high` as an `EXTRACTED`/`DECLARED` class filter, `--label-communities`/`--deep`/`update --embed` flags, missing env-var rows (`ASTRIA_LLM_BUDGET`, `ASTRIA_LLM_COMMUNITY_MAX`, `NEO4J_*`), the real `scip_*` relations replacing the never-emitted `method`/`inherits`/`forks`, schema table columns, memory ingestion timing, and `same_type_as` label-based grouping. The docs-sync guard no longer counts `#[cfg(test)]` fixtures as relation emitters (a clamp-test `relation: "forks"` had been satisfying the check for a documented relation that does not exist).
- **Docs drift guard, both directions** — the docs-sync check now also fails when ARCHITECTURE.md documents a relation that no production code emits or references (with an explicit `(external only)` escape), when an `ASTRIA_*` variable is read but undocumented or documented but never read, when a registered CLI command/flag or MCP tool is missing from its reference page, and when the new generated SQLite schema block is stale. The schema block is generated from `db.rs` into the architecture page by `scripts/generate-schema-docs.mjs` (column lists can no longer drift — the first generated block surfaced five previously undocumented columns); ARCHITECTURE.md's schema section now links there instead of restating columns, and the website's relation list defers to the graph-model reference. The distributed skill (`skills/astria/SKILL.md`) gained an explicit scope note pointing at the full surface.

## [1.0.6] — 2026-09-27

### Chunked document retrieval
- Markdown, text, and RST document bodies are chunked into searchable section nodes (~1200 characters each; `ASTRIA_CHUNK_CHARS` overrides, clamped 400–8000). Extraction rules bump to v10, so the first `astria update .` after upgrading re-extracts documents.
- Consecutive chunks share a line-snapped ~180-character overlap tail, so evidence spanning a chunk boundary surfaces from either side; chunk records cite their covered line range (`span: L14-L18`).
- Chunks are their own `chunk` node type: on code-majority graphs they rank under code and documents so body-term luck cannot displace exact code answers, while doc-only graphs treat them as the corpus. Chunk docstring evidence scores at label parity, with a coverage multiplier rewarding nodes that match most question terms.
- `ASTRIA_EMBED=off` (or `astria query --no-embed`) opts out of query-time embedding seeds while keeping structural queries working. Measured on the full LoCoMo set (1,977 evidence-backed questions, structural only): recall@10 0.2% → 84.5%, recall@1 → 63.5%, MRR → 0.717.

### Added
- **MCP parity on the CLI** — `astria god-nodes`, `astria communities`, and `astria neighbors <node> [--relation R]` answer what the MCP `god_nodes`/`list_communities`/`get_neighbors` tools answer, for scripts and non-MCP agents.
- **`--json` on the query family** — `query`, `map`, `explain`, `path`, `affected`, `stats`, and `status` emit machine-readable results: counts, continuation cursors, full neighbor and blast-radius hit lists, and type breakdowns instead of prose.
- **Graph build provenance, a true freshness probe** — every `run`/`update` stamps the npm CLI version and the extraction-rules version into the graph. `astria status` (text and `--json`) reports when the graph was built, by which astria, under which extraction rules, and whether it predates the installed binary's rules (`extractionOutdated`); a CLI-version change also warns at build time. Graphs built before 1.0.6 report nulls until their next update.

## [1.0.5] — 2026-09-27

### Retrieval correctness and response budgets
- Preserve scoped code/test definitions during semantic deduplication. Extraction cache invalidation lets `astria update .` restore previously lost definitions.
- Extract JS/TS assigned functions with qualified bindings, scope, documentation and callback bodies. Prefer Python concrete implementations over same-scope overload declarations.
- Rank complete identifiers and honor explicit test/documentation intent; classify Rust inline tests from AST attributes and modules.
- Enforce exact `o200k_base` query-text budgets, including metadata, in CLI and MCP. Invalid or insufficient budgets return errors. MCP transport JSON is excluded. Continuation cursors now count node and edge records; discard old cursors after upgrading or rebuilding.
- Add pinned paired benchmarks, grounded symbol diagnostics and ID collision reporting. See [current results and limitations](website/docs/explanation/retrieval-validation.md).

### Added
- **Cross-layer linking** (`astria-build::crosslayer`): three deterministic
  post-build passes bridge layers the per-file extractors cannot see, all
  edges tagged context `crosslayer` and re-derived on every pipeline run —
  docs that name a package get `references` edges to it (the architecture
  crate table now reaches code), packages get `entry_point` edges to their
  conventional entry file, and TS/JS symbols importing the napi binding get
  `ffi_binding` edges to the backing Rust function (camelCase ↔ snake_case).
  Closes the last golden-QA full miss (q07, "how does the MCP server expose
  tools to agents") structurally — the docs→napi→`astria-mcp` chain is now
  traversable — with no golden-set edits.

### Changed
- **Doc-heading cap**: query answers render at most 6 document-type nodes —
  doc headings keyword-match almost anything and could absorb the node
  budget (the second ranked-fix from the golden-QA indictment).
- **`--detail high` prefers file-level nodes** when rendering answers, on
  equal relevance.
- The quality job in the benchmark snapshot workflow is now **blocking**:
  fails when golden-QA recall@5 falls below 50% (`--min-recall5`).
- **Docs-vs-code drift guard** (`scripts/check-docs-sync.mjs`, CI job
  `docs-sync`): asserts every workspace crate and every emitted relation is
  documented in ARCHITECTURE.md, and that README/CONTRIBUTING version
  claims match the workspace metadata. Caught 5 undocumented relations on
  its first run.
- Blind LLM judging (promptfoo) is wired as a workflow job, gated on the
  `PROMPTFOO_JUDGE_KEY` secret.
- **Four new languages**: Terraform/HCL, PowerShell, Verilog/SystemVerilog,
  and Metal shaders (via the C++ grammar) — 21 -> 25.
- **`astria callflow <node>`**: Mermaid `flowchart` of the calls around a
  node (`--depth`, `--direction in|out|both`), rendered natively by GitHub
  and Obsidian.
- **FalkorDB export**: `export --format falkordb` writes openCypher with
  load instructions; `--redis-push host:port` loads it live via redis-cli
  (alongside the maintainer-added SVG export and live Neo4j push).
- Video/audio ingestion via Whisper transcription is scoped and tracked
  in issue #82 (external-binary mode recommended).
- Query seed selection caps documentation-type seeds at 2 of 5 and scores
  directory/crate-name path matches above bare substrings; the self-corpus
  ignores `worked/` via `.astriaignore`. Measured on the golden set:
  MRR 0.537 -> 0.576, recall@5 62.9% -> 71.4%, recall@10 85.7% -> 88.6%.
  The three former full misses (tree-sitter language support, MCP tool
  exposure, LLM semantic enrichment) now surface their implementing files.
- **Phrase bonus in seed scoring**: consecutive question tokens appearing
  verbatim in a label or docstring ("blast radius") lift the node —
  token-level scoring treated the words as unrelated and lost to weaker
  lexical-luck matches (fixes the q09 blast-radius regression).
- **Same-named files in different directories are separate entities**:
  dedup no longer merges file-shaped nodes at all (transitive union chains
  had merged `src/index.ts` into `install/index.ts`, erasing the CLI entry
  file). The guard prevents future merges; healing the already-scarred
  store additionally required a full rebuild, because the incremental
  pipeline only re-extracts changed files and never restored the deleted
  entry node on its own.
- **IDF-weighted seed scoring**: a term that matches a large share of node
  labels ("index" hits every index.* file) is scaled down, while rare terms
  ("scip") keep full strength — previously the generic matches won by
  alphabetical tie-break and flooded the seed set (fixes the q26 SCIP miss).
- **Entry-point intent**: questions asking for the entry point are answered
  structurally — a file that imports many modules and is imported by none
  (tests excluded) is the program's front door, whatever its filename —
  because no lexical hook can find an entry file named `index.ts` (fixes
  the q17 CLI-entry-point miss). Measured on the golden set after the
  rebuild + scoring work: MRR 0.592 -> 0.690, recall@1 45.7% -> 57.1%,
  recall@5 80.0% -> 88.6%; one question remains a full miss (q07, the MCP
  server crate sits across a prose-to-code layer gap the traversal does
  not bridge).
- **Benchmark re-pinned to upstream graphify v0.9.69** (first release with
  a query-capable CLI): the blind answer-quality comparison now compares
  against answers instead of errors. The speed/density corpus changes
  accordingly; historical numbers remain labeled at their original pins.
- **File ids are collision-free across a workspace**: extraction rooted node
  ids at the last path directory only, so every crate's `src/lib.rs`
  produced the same `src_lib` id and build treated the later crates' lib.rs
  as cross-file merges of the first — one hub node absorbed the whole
  workspace's `contains` edges (degree 400+ in the self graph). Stems now
  join every path component; flat layouts keep their old ids.
- **Thematic community labels**: communities are named after their most
  distinctive term ("Extract", "Similarity") — the token that concentrates
  inside the community relative to the whole graph — instead of the
  highest-degree member's name ("get()", "lib.rs"). Sub-support communities
  keep the deterministic hub label; naming is a pure function of the graph.
- `learned`-edge regeneration drops every learned edge, not only rows
  stamped `query_history` — a stale row from an older convention no longer
  survives beside its regenerated twin.
- Self-corpus ignore additions: `website/versioned_docs/` (frozen docusaurus
  copies duplicate the live docs byte-for-byte and split clusters against
  their live twins) and `crates/astria-napi/tests/fixtures/` (parser
  fixtures held community slots without aiding orientation).

### Added
- **Measured, capped, cached LLM enrichment**: every backend response's
  usage block is counted (OpenAI/Anthropic/Gemini wire formats), the run
  summary prints `LLM usage: N API calls, X in / Y out tokens` plus the
  cache-hit count, and `pipeline_runs` persists the spend per run.
  `ASTRIA_LLM_BUDGET` caps a run's total tokens; remaining files fail
  loudly instead of silently degrading.
- **`run --label-communities` / `update --label-communities`**: thematic
  community naming with one LLM call per *changed* community (membership
  fingerprint cached in `communities.member_hash`; rebuilds preserve LLM
  labels while membership is unchanged and drop them when it drifts).
  Labels carry a one-line summary and explicit provenance
  (`communities.label_source`: `llm` vs `hub`), surfaced in MCP
  `list_communities`, `graph_report.md`, the new `communities` array in
  `graph.json`, and exports. `ASTRIA_LLM_COMMUNITY_MAX` caps calls per run
  (default 48, largest communities first); communities under 3 nodes keep
  deterministic names.
- **`run --deep` / `update --deep`**: second extraction tier — one LLM call
  per file links the file's code symbols to concept nodes from other files
  as `INFERRED` edges (`context='deep'`), the cross-file concept mesh the
  AST cannot see. Cached per file content hash; stale links are replaced
  idempotently on rebuild.
- **`.github/copilot-instructions.md` injection** for `install copilot`:
  the managed section now lands in Copilot's native custom-instructions
  file in addition to AGENTS.md (parity with upstream's installer).
- **`astria health`** (and MCP `health` tool): a scored code-health report —
  unreachable-symbol candidates (call-graph heuristic; entry points, test
  files, and file-shaped nodes excluded), circular file dependencies
  (SCC over calls/imports), hub concentration, and graph staleness, with
  the deduction schedule printed inline.
- **`astria risk`**: maps the current `git diff` (or `--staged`) onto the
  graph via reverse reachability and renders a PR-ready report — impacted
  symbols by depth, communities touched (labels included), review focus,
  and a documented heuristic score for CI triage.
- **`export --format svg`**: deterministic community-arc SVG (dark theme,
  hub labels, XML-escaped) — byte-identical across runs, graceful caps at
  2k nodes / 6k edges.
- **`export --format cypher --neo4j-push <url>`**: live Neo4j push over a
  hand-rolled Bolt client (new `astria-bolt` crate: PackStream + chunked
  framing + HELLO/RUN/PULL, mock-server tested, zero driver dependencies).
  Parameterized UNWIND batches; idempotent MERGEs; communities pushed as
  first-class nodes.

### Fixed
- Build validation rejected the pipeline's own `SEMANTIC` edge confidence
  (added by LLM enrichment) — `run` with any semantic backend failed
  wholesale at build time. `SEMANTIC` is now part of the accepted
  vocabulary (query ranking already treats it above `INFERRED`).

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

[1.0.6]: https://github.com/Nodesify/astria/compare/v1.0.5...v1.0.6
[1.0.5]: https://github.com/Nodesify/astria/compare/v1.0.4...v1.0.5
[1.1.0]: https://github.com/Nodesify/astria/compare/v1.0.12...v1.1.0
[1.0.12]: https://github.com/Nodesify/astria/compare/v1.0.11...v1.0.12
[1.0.11]: https://github.com/Nodesify/astria/compare/v1.0.10...v1.0.11
[1.0.10]: https://github.com/Nodesify/astria/compare/v1.0.9...v1.0.10
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
