# PLAN: multimodal ingestion, global graph, hypergraph, ecosystem plumbing

Ports the four feature areas the official graphify v0.9.67 has that we lack, mapped onto
our architecture (Rust crates + SQLite + thin napi boundary + TS CLI). Grounded in the
official's actual implementation (researched from `C:\Nodesify\graphify` @ v8).

## Non-negotiables (apply to every phase)

- **Zero API keys, fully offline** by default. Anything needing a live network (Postgres
  DSN, `gws` CLI, whisper model download) is opt-in by flag and clearly marked.
- **SQLite is the store.** No graph.json sprawl; JSON only as an export target.
- **napi boundary stays thin** — logic lives in crates, `graphify-napi` just bridges.
- Schema changes bump `_meta.schema_version` (currently 2 → 3) with a migration.
- Every phase ships tests + changelog entry. Boring over clever.

## Recommended order

Plumbing → Hypergraph → Ingest → Global graph. Plumbing is quick wins that also
de-risk later phases (validate/diagnose help debug the big ones). Global graph is last:
largest surface, touches query commands and schema most.

---

## Phase 1 — Ecosystem plumbing (S items first)

### 1.1 Graph validation (S)
Port of `validate.py`. New `graphify-core::validate`:
- Before graph assembly in `run_pipeline`/`update_pipeline`: every node has
  id/label/file_type/source_file; every edge has source/target/relation/confidence and
  **endpoints that exist as nodes**; confidence ∈ {EXTRACTED, INFERRED, AMBIGUOUS}.
- Fail loud with a list of all violations (not first-only).
- **AC:** corrupted extraction (test fixture with a dangling edge) fails the run with a
  readable error naming the edge.

### 1.2 `diagnose` command (S)
Port of `diagnostics.py`, read-only, works on any `.graphify` dir:
- Counts: dangling edge endpoints, self-loops, duplicate edges, unclassified files
  (from detect), stub vs actionable danglers, communities with 0 cohesion.
- `--json` machine output. No mutation.
- **AC:** `nodesify-graphify diagnose .` on this repo prints a clean report; `--json`
  parses.

### 1.3 Query log (S)
Port of `querylog.py`. We already record `query_history` in SQLite; add the
env-gated JSONL for agent/tooling consumption:
- `GRAPHIFY_QUERY_LOG=<path>` or `=1` (default `~/.cache/nodesify-graphify-queries.log`);
  `GRAPHIFY_QUERY_LOG_DISABLE=1` wins. Append ts/kind/question/nodes/duration_ms.
- Fail silent, never break a query.
- **AC:** env var set → file grows by one line per query; unset → nothing.

### 1.4 `hook-guard` (M) — editor PreToolUse nudges
Port of official hook-guard, **nudge mode first, strict opt-in**:
- `hook-guard search`: on Grep/grep-like Bash call with a graph present, emit
  `additionalContext` nudge to run `query` first. Never blocks.
- `hook-guard read`: skip non-source exts + `.graphify/` paths; staleness nudge (file
  mtime > graph mtime); strict mode (opt-in flag baked at install, env override) can
  deny ONE un-indexed read per session, suppressed for 30 min after any
  query/explain/path (stamp file in `.graphify/cache/`).
- Installed into `.claude/settings.json` PreToolUse with `"timeout": 10`; fails open on
  any error (print nothing, exit 0).
- We already have `hook install` for git post-commit; this adds the editor side.
- **AC:** installed settings survive re-install idempotently; guard adds <200 ms to a
  tool call; a query resets the strict window.

### 1.5 `save-result` + `reflect` (M) — feedback loop, doc-driven
Port of save-result/reflect:
- `save-result --question Q --answer A [--outcome useful|dead_end|corrected]
  [--correction T]` → `graphify-out/memory/query_<ts>_<slug>.md` with YAML frontmatter.
- `update` ingests memory docs as `rationale`/`concept` nodes (docs.rs frontmatter
  parse; the parse_memory_doc shape is frontmatter + `# Q:`/`## Answer`/`## Outcome`).
- `reflect` aggregates outcomes → `graphify-out/reflections/LESSONS.md`.
- Complements our learned-edge promotion (query_pairs): theirs is curated, ours is
  automatic — both feed the same compounding story.
- **AC:** save → update → the Q appears as a graph node linked to `source_nodes`;
  reflect renders LESSONS.md with outcome tallies.

### 1.6 always-on instruction blocks (S)
Port of `always_on/*.md` fragments: `install` (and `install claude` etc.) inject a
marker-anchored `## graphify` section into `AGENTS.md`/`CLAUDE.md` (we already do
AGENTS.md — extend to CLAUDE.md and add the two key behaviors: *query before grep*,
*run `update .` after code edits*). Idempotent, respects existing content.
- **AC:** double install leaves one block; uninstall removes it.

---

## Phase 2 — Hypergraph with a deterministic producer (M)

Official hyperedges are LLM-produced only (docs/media chunks, max 3 per chunk). Ours
gets a **local, deterministic producer** — this is our differentiator, not a port.

### Design
- Schema: `hyperedges` table (id, label, nodes TEXT/json array, relation, confidence,
  confidence_score, source_file). Version bump 2 → 3.
- Producers (both run in `cluster`/build, deterministic):
  1. **Community hyperedges** — each community ≥ 3 nodes: `id = community::<id>`,
     relation `participate_in`, nodes = top-8 by degree (label = community label).
  2. **Reference hyperedges** — every `str::` reference node with ≥ 3 inbound
     `references` edges from distinct files: `id = hyper_ref::<literal>`,
     relation `shares_reference`, nodes = referencing files + the literal.
- Consumers: `graph.json` export (`hyperedges` array, schema-compatible with official),
  GRAPH_REPORT section, HTML viewer shaded convex hull over member nodes (standard
  mode; large mode draws label circles), wiki index section, `explain` shows membership.
- Skip (deliberate): LLM-produced N-ary relations — revisit only if a real need appears.
- **AC:** run on this repo produces ≥ 190 community hyperedges + reference hyperedges;
  graph.json validates against the official consumer shape; HTML renders hulls without
  breaking the large-mode viewer.

---

## Phase 3 — Ingest breadth (offline subset first)

Official research: URL ingest writes markdown sidecars; SCIP/PG/Cargo/manifest/.mcp are
local; whisper is local-but-heavy; Google Workspace shells to `gws` + auth.

### 3.1 Cargo introspection (S)
`--cargo` on `run`/`update` (or auto when Cargo.toml present — we already ingest
Cargo.toml as a manifest; this adds **workspace member + internal path-dep topology**):
nodes `crate:{name}`, edges `crate_depends_on` (workspace-internal path deps only,
honoring `package =` renames and `workspace = true` inheritance). Pure `toml` parsing.
- **AC:** on a workspace repo, member crates link to each other.

### 3.2 MCP config ingest (S)
Auto-route `.mcp.json`, `mcp_servers.json`, `claude_desktop_config.json` (1 MiB cap):
`mcp_server`/`mcp_command`/`mcp_package` nodes + `contains`/`references`/
`requires_env` edges (env **names only**, values never read). Highly agent-relevant.
- **AC:** a repo with `.mcp.json` exposes its servers as queryable nodes; env values
  never appear in the graph (test asserts).

### 3.3 SCIP-ish JSON ingest (M)
`add --scip <file>` accepting the simplified SCIP-style JSON (documents/symbols/
relationships — same shape the official accepts, not protobuf): `scip_impl`/`scip_typed`/
`scip_def`/`scip_ref` edges, stub nodes for unresolved symbols, deterministic ids from
sha1(path:symbol). Brings rust-analyzer/SCIP-producing toolchains into the graph.
- **AC:** a hand-built SCIP fixture round-trips into nodes+edges; rerun is idempotent.

### 3.4 Postgres introspection (M, opt-in)
`--postgres <DSN>`: read-only introspection of information_schema (tables/views/
routines/FKs) → nodes under a virtual file id `postgres://{host}/{db}` with
`contains` + `references` (FK) edges. Emit nodes **directly** (skip the official's
SQL-grammar detour — we don't ship a SQL grammar). New dep: a small sync PG client
(`postgres` crate), feature-gated so default builds stay lean.
- **AC:** against a scratch DB, FK graph matches schema; no credentials stored anywhere.

### 3.5 Transcript sidecar ingestion (S)
Any `.txt`/`.md` under `graphify-out/transcripts/` becomes a `document` node set via
the existing markdown extractor — the *contract* of the official's whisper step, minus
the model. **Local whisper itself: deferred** (whisper-rs would bloat our prebuilt
binaries the way fastembed already forced a darwin-x64 gate). Document the contract so
users can run any transcriber.
- **AC:** dropping a transcript file in and running `update` yields a document node
  linked to the repo.

### Deferred deliberately
- **Audio/video transcription models** (binary bloat; sidecar contract covers it).
- **Google Workspace** (`gws` CLI + OAuth; revisit on request).
- **YouTube/tweet URL ingest parity** — our `ingest_url` already classifies and writes
  sidecars; extend classification incrementally as needed.

---

## Phase 4 — Cross-repo global graph (L)

Port of `global_graph.py` + `cross_repo_calls/types.py`. All local, no LLM.

### Design
- **Global store**: `~/.nodesify-graphify/global.db` (SQLite, same schema) +
  `global-manifest.json` (per-repo: tag, added_at, source hash, node/edge counts).
- **`global add <root> [--as <tag>]`**: build/open the repo graph, then merge with
  **repo-tag prefixing**: every sourced node id becomes `<tag>::<local_id>`; node attr
  `repo = tag`; **external/stub nodes stay unprefixed and dedupe by label** so
  `serde_json::Value` or `typing` unifies across repos. Hash-unchanged repos skip.
  Re-add prunes and replaces that repo (idempotent). `global remove <tag>`,
  `global list`, `global path A B`.
- **`merge-graphs` parity**: rewrite our naive `merge.rs` to namespace-aware merging
  (both inputs prefixed) instead of bare id-dedup.
- **Cross-repo type edges**: `same_type_as` (INFERRED 0.9) between type declarations
  sharing `(namespace, label)` across ≥ 2 repos, pairwise, skip existing.
- **Cross-repo calls**: extraction already drops unresolved calls; add parking —
  store `unresolved_calls` (lang, receiver_type, callee, line) as node metadata
  (schema addition). Global pass then resolves a parked call when **exactly one**
  candidate receiver-type declaration exists in a *different* repo with exactly one
  matching member — start with Rust + TS/JS (our strongest resolvers), fail closed on
  ambiguity, marker column so the pass is re-runnable.
- **Query surface**: `query`/`explain`/`path` gain `--graph <path-to-global-db>`
  (commands are already graph-agnostic underneath).
- `run --global --as <tag>` auto-adds after build.
- **AC:** two fixture repos sharing a type name produce `same_type_as`; a parked
  cross-repo method call resolves to the single candidate; `global add` twice is
  idempotent (hash skip); `global remove` prunes cleanly; queries run against the
  global store via `--graph`.

---

## Cross-cutting checklist per phase

- [ ] Schema version bump + migration (phases 2, 4; 3 only if metadata parking lands early)
- [ ] napi exports + TS command registration
- [ ] README "What's new" + CHANGELOG line
- [ ] Unit tests in-crate + one CLI-level e2e per feature (pattern: `src/__tests__/e2e.test.ts`)
- [ ] TODO.md tick-off
