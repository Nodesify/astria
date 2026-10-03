# Graphify vs astria — supporting-feature gap analysis

**Date:** 2026-10-03 · **Repo:** `Nodesify/astria` v1.0.11 (+ Unreleased #82)
**Upstream reference:** [Graphify-Labs/graphify](https://github.com/Graphify-Labs/graphify) — Python, latest observed release **v0.9.73 (Sep 30, 2026)**, PyPI package `graphifyy`.
**Lineage note:** astria's README/`website/docs/intro.md` state it is *inspired by* Graphify's core idea but an independent from-scratch implementation (not a fork); the legacy `GRAPHIFY_*` env-var fallback (`crates/astria-core/src/lib.rs:31–38`) and the head-to-head benchmark vs upstream commit `91f4d12` (`website/docs/explanation/benchmarks.md:40`) document the lineage.

Confidence legend: ✅ confirmed by multiple/live sources · ⚠️ single-source (graphify.net CLI reference or raw-file fetch with CDN skew) · ❓ unconfirmed detail.

---

## Part 1 — Missing supporting features (the list)

### A. Ingestion & language coverage

| # | Gap | What graphify has | What astria has today | Evidence |
|---|-----|-------------------|-----------------------|----------|
| A1 | **~11 code languages / grammars** | 36 tree-sitter grammars, incl. Vue, Svelte, Astro, Julia, Fortran, VB.NET, Solidity, R, Objective-C, Groovy/Gradle, Luau — plus regex-extras for OCaml, Common Lisp, Pascal/Delphi, BYOND DreamMaker ✅ | 25 languages (`website/docs/reference/language-support.md`, generated from `crates/astria-core/src/languages.rs`). Verilog `.v/.sv` overlaps SystemVerilog; Lua exists but not Luau | graphify.com FAQ + README (live) |
| A2 | **SQL schema ingestion** | `sql` extra: tree-sitter-sql over schema files, incl. `CREATE TRIGGER` extraction (v0.9.71) ✅ | None — no `.sql` classification or extraction | release notes (live) |
| A3 | **Office documents (.docx, .xlsx)** | `office` extra (python-docx, openpyxl) ✅ | None | README extras (live) |
| A4 | **Google Workspace ingestion** | `.gdoc .gsheet .gslides` via `gws` auth + `--google-workspace` flag; exports to `graphify-out/converted/` Markdown sidecars ✅ | None | README (raw, v8-era) |
| A5 | **Docs: HTML, YAML, Quarto** | `.html .yaml .yml .qmd` treated as documents ✅ (⚠️ raw-fetch, v8-era table) | Documents = `.md .mdx .txt .rst` only (`crates/astria-detect/src/lib.rs` `DOC_EXTENSIONS`) | README file-types |
| A6 | **Video/audio URL ingestion** | `yt-dlp` in `video` extra — YouTube / arbitrary video URLs transcribed ✅ | `add <url>` covers tweet/arXiv/PDF/image/webpage only (`crates/astria-ingest/src/lib.rs` `UrlKind`); local media files only after #82 | README (live), pyproject |

### B. Platform & integration surface

| # | Gap | What graphify has | What astria has today | Evidence |
|---|-----|-------------------|-----------------------|----------|
| B1 | **MCP over HTTP + team serving** | stdio **and** HTTP transports (`python -m graphify.serve`), `graphify-mcp` entry point, multi-project MCP serving ✅ | stdio only (`astria mcp --graph`, one graph per process, `crates/astria-mcp/src/lib.rs`) | pyproject + FAQ (live) |
| B2 | **First-class Bedrock / Azure / Kimi backends** | Named backends: AWS Bedrock (IAM), Azure OpenAI, Kimi/Moonshot, DeepSeek, Ollama ✅ | Claude, Gemini, OpenAI-compatible (covers DeepSeek/Ollama/LM Studio/custom); Bedrock/Azure have no first-class auth path (Bedrock SigV4 can't ride the OpenAI client) | README env table; `crates/astria-semantic/src/lib.rs` |
| B3 | **Rich PR dashboard** | `graphify prs`: CI state, review status, worktree mapping, AI-ranked review queue, merge-order risk; `--triage`, `--conflicts` ✅ | `astria prs [count] --conflicts` (requires `gh`) — thinner | README (live) vs `packages/astria-cli/src/index.ts` |
| B4 | **Git merge driver for the graph file** | Union-merge driver merges `graph.json` on parallel branch commits ✅ | `astria merge A B out` exists, but no gitattributes merge driver → parallel commits conflict on `.astria/db.sqlite` | README (raw) |
| B5 | **Docker distribution** | Dockerfile in repo root ✅ (published image ❓) | None (npm + Homebrew tap + skills.sh + plugin marketplace + MCP registry + Smithery) | repo contents (live) |
| B6 | **Hosted / enterprise tier** | app.graphify.com Free/Pro/Teams/Enterprise, hosted MCP, merge-gate verification, graph-aware review, engineering digest, Jira mention ✅ | None (pure OSS/local) | graphify.com FAQ (live) |

### C. Query & graph features

| # | Gap | What graphify has | What astria has today | Evidence |
|---|-----|-------------------|-----------------------|----------|
| C1 | **CJK query segmentation** | `chinese` extra (jieba) for Chinese-language queries ✅ | No CJK tokenization in the hybrid retrieval engine | README extras |
| C2 | **"Why" nodes (rationale extraction)** | NOTE/WHY/HACK comments + docstrings + docs rationale surfaced as dedicated nodes ✅ | docstrings stored per node, but no dedicated WHY-node pass | README "What's in the report" |
| C3 | **Clustering controls** | `--cluster-only --resolution <r> --exclude-hubs` ✅ | `cluster-only` with no flags; fixed label-propagation parameters | README vs `packages/astria-cli/src/index.ts` |
| C4 | **Cost report artifact** | `cost.json` per run ⚠️ | Token usage recorded in `pipeline_runs` table, but no surfaced cost report file | README (raw) vs directory-layout docs |
| C5 | **Deep-clean uninstall** | `uninstall --purge` ✅⚠️ | `uninstall` (platform configs); no purge-everything flag | graphify.net (affiliation unverified) |
| C6 | ⚠️ Single-source flags: `--mode deep`, `--obsidian`, `--graphml` | Reported only on graphify.net (affiliation unconfirmed) | astria already has `--deep`, `wiki --format obsidian`, `export --format graphml` → likely parity, verify before building | graphify.net CLI reference |

---

## Part 2 — Already at parity (don't rebuild)

| Area | graphify | astria |
|------|----------|--------|
| PDF / images / local video+audio | pdf extra, vision, faster-whisper | `astria-pdf`, vision backends, `astria-audio` (#82, external whisper-cli) |
| URL ingestion (tweet/arXiv/PDF/image/webpage) + `--author/--contributor` tags | ✅ | ✅ identical surface |
| PostgreSQL introspection | `--postgres DSN` | `add --postgres <dsn>` |
| SCIP code-intel edges | — (not found upstream) | `add --scip` (astria **ahead**) |
| Interactive HTML / GraphML / SVG / Neo4j push / FalkorDB / callflow / wiki / Obsidian | ✅ | ✅ (astria adds symbol `tree`, redis-push, `global` graph) |
| Watch mode, post-commit **and** post-checkout hooks | ✅ | ✅ (`watch --debounce`, `hook install` v4 + `hook-guard`) |
| Merge graphs CLI | `merge-graphs` | `merge` / `diff` |
| God nodes, communities, surprising connections, suggested questions, `GRAPH_REPORT`/`graph_report.md` | ✅ | ✅ + blast radius |
| `--wiki`, `--update` incremental, ignore-file with `!` negation, headless CI | ✅ | ✅ (`.astriaignore`, `update --if-stale`) |
| MCP query_graph/neighbors/shortest_path + stats/health tools | 10 tools | 10 tools (`crates/astria-mcp`) |

## Part 3 — Where astria is ahead

Local fastembed embeddings + `similar_to` edges (graphify has **no** vector store by design) · richer provenance tiers (RESOLVED/SEMANTIC/DECLARED + calibrated scores vs EXTRACTED/INFERRED/AMBIGUOUS) · learned edges + `save-result`/`reflect` memory · `risk`/`diagnose`/`health` scoring · Jev judge calibration layer · global cross-repo graph · 5-platform native npm binaries + Homebrew + plugin marketplace + MCP registry + Smithery · deterministic (non-LLM) core pipeline.

## Suggested priority (impact × effort)

1. **A2 SQL ingestion** — high user value, tree-sitter grammar already exists upstream-proven.
2. **A1 top grammars: Vue, Svelte, Astro, Solidity, Groovy** — web-dev coverage gap.
3. **B1 MCP HTTP transport** — unlocks team/CI serving astria has no answer for.
4. **A3 Office docs** — common in enterprise corpora.
5. **B4 git merge driver** — small, removes real team friction.
6. **B3 PR dashboard depth**, **A6 video URLs**, **C1 CJK segmentation** — larger efforts.

---

*Method: two parallel researchers (web + repo, live-fetched GitHub API/releases/PyPI/graphify.com on 2026-10-03); full reports at `.zwork/runs/33990174/0-研究员.md` and `.zwork/runs/33990174/1-研究员.md`. Known caveats: upstream raw-CDN skew (raw pyproject showed 0.9.46 vs live 0.9.73 — live sources preferred); graphify.net-only flags unverified; upstream license sources conflict (Apache-2.0 best-attested vs "MIT" marketing claim).*
