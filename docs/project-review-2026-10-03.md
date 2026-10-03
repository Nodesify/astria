# Project review and fix backlog — 3 October 2026

This review found **36 source-backed defects or implementation gaps**, followed by **6 engineering improvements**. The highest priorities are protecting secrets and exported HTML, correcting risk scoring, preventing graph identity/data-loss problems, and repairing build distribution.

This is a broad source review, not a claim that every possible defect has been discovered. Findings describe reachable code paths; security attacks and external integrations were not executed. Priorities account for the conditions under which each feature is used.

## Status (updated after implementation)

All 42 findings below were implemented and reviewed across commits `22c0482` (backlog implementation), `e12af80` (deferred-media regression fix), `72c469a` (documentation), and the follow-up review work of the same day. Status legend: **resolved-tested** = implemented with unit/integration regression tests; **resolved-runtime** = implemented, and validated only where noted (live external service or manual assistive-tech validation not performed in CI).

| Finding | Status | Notes |
|---|---|---|
| F01, F02, F06, F08, F09, F16, F19, F25, F27, F29, F30, F32, F33, F35, R01, R03, R04, R06 | resolved-tested | verified in the first implementation pass |
| F04, F05, F07, F10, F11, F12, F13, F15, F20, F22, F23, F24, F26, F28, F31, F36, R02 | resolved-tested | completed in the second pass; R02's snapshot cache is now a bounded multi-project LRU (see follow-up item U4) |
| F03 | resolved-runtime | `+s`/`+ssc` ride rustls with scheme-preserving transport; no live Neo4j TLS server in CI |
| F14 | resolved-tested | cross-repo id disambiguation on merge |
| F17 | resolved-runtime | Docker manifests restored; image build exercised in release CI only |
| F18 | resolved-tested | viewer asset path fixed; CI bundle-drift guard added |
| F21 | resolved-tested | pending-retry mechanism; the deferred-media regression it introduced was caught by the version A/B benchmark and pinned with `unextractable_binaries_do_not_keep_the_graph_dirty` |
| F34 | resolved-runtime | keyboard/screen-reader navigation improved; manual screen-reader pass still recommended |
| R05 | resolved-runtime | compatibility removals per project policy |

### Follow-up review (later, 3 October 2026)

A second review pass found six further issues (U1–U6). U1–U4 and U6 are implemented in the same follow-up change set; U5 is partially done — the reporting side landed, the corpus-growth work remains open.

| # | Finding | Status | Notes |
|---|---|---|---|
| U1 | Cache invalidation not part of graph publication; cluster-only changed communities without advancing the generation or republishing artifacts | resolved-tested | the generation now advances inside the publication transaction and after every committed content change (dedup, deep links, embeddings, learned edges, community memberships, labels); unchanged runs reuse the previous generation; `cluster_only` publishes through the shared artifact workflow |
| U2 | Freshness verified by timestamps only (merge gate; query warnings) | resolved-tested | merge gate compares the recorded git HEAD (`_meta.git_head`) with the current HEAD and re-hashes every manifest file; query headers report source drift (modified/deleted/size-changed) separately from graph age |
| U3 | Health-score heuristics: containment counted as reachability; hubs flagged without thresholds | resolved-tested | usage/non-usage relation split; hubs need degree >= max(10, p95) with test-file hubs reported instead of scored |
| U4 | Process-wide snapshot cache held one graph; multi-project MCP thrashed | resolved-tested | bounded LRU keyed by path + generation (default 3, `ASTRIA_SNAPSHOT_CACHE_ENTRIES` 1..=16) |
| U5 | Retrieval evidence: track exact-symbol ranking alongside file recall and tokens; grow held-out corpora | partial | `scripts/bench/paired/report.mjs` now emits the metric trio plus a per-case definition-miss triage list, and `retrieval-validation.md` ranks the known failure modes; authoring genuinely held-out corpora remains open |
| U6 | Documentation drift (language counts, cache claims, backlog statuses) | resolved | registry-derived count claims drift-checked by `scripts/check-docs-sync.mjs`; stale hand-maintained list removed; this status section added |

## Scope and verification

Reviewed the Rust workspace and TypeScript CLI across discovery/extraction, persistence, graph identity, queries, merges/global stores, semantic enrichment, URL/media/Office/Workspace ingestion, MCP transports, HTML exports/viewer, Docker, release/CI, and documentation. Existing architecture documentation and `.astria/graph_report.md` provided orientation. Graph queries reported **76 changed files**, so conclusions were checked against current source rather than accepted from the graph.

Compilation completed successfully for `cargo check --workspace --locked`, `npm run build --workspace @nodesify/astria`, `npm run typecheck --workspace @astria/viewer`, and `npm run build` in `website/`. No unit, integration, end-to-end, or exploit tests were run, following the project instructions. No implementation fixes were made.

Priority meanings: **P1** = fix before relying on the affected feature in production or releasing it; **P2** = next implementation backlog; **P3** = engineering/documentation improvement. An optional feature's P1 rating does not imply every installation is exposed.

## Security and trust boundaries

### F01 · P1 · HTML exports allow mixed-case script terminators

**Evidence:** [export_html.rs](/C:/Nodesify/nodesify-graphify/crates/astria-export/src/export_html.rs:130).

Embedded graph JSON replaces lowercase `</script` only. HTML recognizes mixed-case closing tags too, so a label containing `</SCRIPT><script>…</script>` can terminate the data script and introduce executable markup when someone opens the exported graph. JSON serialization alone does not make a string safe inside an HTML script element.

**Fix:** Escape every `<` in embedded JSON as `\u003c`; keep graph content in text/canvas APIs. Review every inline data insertion against the same rule.

### F02 · P1 · LLM enrichment can send MCP configuration credentials

**Evidence:** [manifest allowlist](/C:/Nodesify/nodesify-graphify/crates/astria-core/src/types.rs:64), [semantic candidate collection](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/semantic_pass.rs:127), [raw content reading](/C:/Nodesify/nodesify-graphify/crates/astria-semantic/src/lib.rs:234).

MCP config files are intentionally discovered. Their deterministic extractor preserves environment variable names, but the semantic pass separately reads the original file and sends its entire contents to the configured LLM backend. With a remote backend, literal `env` credentials can leave the machine despite the names-only extraction policy.

**Fix:** Exclude credential-bearing MCP configuration from raw semantic processing, or supply a deliberately sanitized representation shared with the deterministic extractor. Apply this before judge gating as well as engine extraction.

### F03 · P1 · `bolt+s://` silently uses plaintext TCP

**Evidence:** [URI parser](/C:/Nodesify/nodesify-graphify/crates/astria-bolt/src/lib.rs:39), [connection](/C:/Nodesify/nodesify-graphify/crates/astria-bolt/src/lib.rs:102).

The parser accepts the secure URI scheme but discards its encryption meaning; the client always connects with `TcpStream` and sends authentication in HELLO. A TLS-only server fails, and a plaintext-capable endpoint can receive credentials without the protection requested by the caller. Neo4j defines `+s` as encryption with certificate verification. [Neo4j SSL framework](https://neo4j.com/docs/operations-manual/current/security/ssl-framework/).

**Fix:** Preserve the scheme and implement verified TLS using an established client, or explicitly reject secure schemes until supported. Never downgrade them silently.

### F04 · P1 · HTTP MCP can exhaust threads and memory before authentication

**Evidence:** [request parser](/C:/Nodesify/nodesify-graphify/crates/astria-mcp/src/http.rs:113), [connection loop](/C:/Nodesify/nodesify-graphify/crates/astria-mcp/src/http.rs:345).

Every accepted connection creates a thread. Request/header lines have no byte limits, sockets have no read/write deadlines, and authentication occurs after the body is read. The 10 MB body cap does not bound headers, concurrent bodies, or clients that never finish a request. This affects network-reachable servers even when a token is configured.

**Fix:** Use a maintained HTTP transport with bounded concurrency, header/body limits, request deadlines, and authentication before allocating/reading large bodies.

### F05 · P1 · Media downloads bypass the URL fetcher's SSRF controls

**Evidence:** [media dispatch](/C:/Nodesify/nodesify-graphify/crates/astria-ingest/src/lib.rs:84), [yt-dlp invocation](/C:/Nodesify/nodesify-graphify/crates/astria-audio/src/lib.rs:389).

Media URLs receive the initial string-level check, then go directly to yt-dlp. They never enter `fetch_bytes`, where DNS answers, redirects, download sizes, and request deadlines are checked. A hostname resolving internally or a redirected media URL is outside the advertised SSRF protection. The external command also has no total execution/download bound here.

**Fix:** Apply one explicit network safety policy to every ingestion route, including external downloaders. Constrain resolved destinations and redirects through a controlled fetch/proxy boundary; enforce media size and execution limits.

### F06 · P2 · URL DNS validation is separate from the actual connection

**Evidence:** [resolved-host validation](/C:/Nodesify/nodesify-graphify/crates/astria-ingest/src/lib.rs:620), [fetch loop](/C:/Nodesify/nodesify-graphify/crates/astria-ingest/src/lib.rs:224).

The code checks DNS addresses, then ureq resolves again when connecting. The source itself documents the DNS rebinding race. Checking redirects does not close that gap.

**Fix:** Connect through the vetted address set while retaining the original Host/SNI identity; use the HTTP library's resolver/connector capability rather than a second independent lookup.

### F07 · P1 · Local HTTP MCP lacks Origin/Host protection

**Evidence:** [HTTP routing](/C:/Nodesify/nodesify-graphify/crates/astria-mcp/src/http.rs:258).

Requests are checked for the optional bearer token, but no Origin or Host allowlist exists. Default loopback mode has no token. This leaves the local graph service without the transport's DNS rebinding defense; exact browser exploitability depends on browser/network protections. Origin validation is required by the advertised MCP transport specification. [MCP 2025-06-18 transport security](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports#security-warning).

**Fix:** Enforce appropriate local/remote Host and Origin policies in the HTTP transport. Validate present origins, reject invalid origins, and keep non-browser clients supported without treating an absent Origin as a browser origin.

## Risk analysis and merge gates

### F08 · P1 · Normal source paths do not match risk-analysis paths

**Evidence:** [risk file selection](/C:/Nodesify/nodesify-graphify/crates/astria-analyze/src/risk.rs:49), [node lookup](/C:/Nodesify/nodesify-graphify/crates/astria-analyze/src/risk.rs:83), [absolute pipeline corpus](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/graph_update.rs:66), [stored extraction path](/C:/Nodesify/nodesify-graphify/crates/astria-extract/src/walkers.rs:1060).

Git returns repository-relative paths; ordinary pipeline extraction stores absolute source paths. `compute_risk` uses exact equality with the relative Git path. Consequently, changed code can have no selected symbols and produce a zero risk score.

**Fix:** Normalize changed paths against the canonical project root using the same path representation as persistence. Also use Git's NUL-separated output so quoted Unicode/space-containing filenames are not misinterpreted.

### F09 · P1 · The CI merge gate does not evaluate the PR's committed diff

**Evidence:** [git_changed_files](/C:/Nodesify/nodesify-graphify/crates/astria-analyze/src/risk.rs:49), [merge gate risk invocation](/C:/Nodesify/nodesify-graphify/packages/astria-cli/src/commands/merge-gate.ts:117).

Risk inspects `git diff HEAD` or `--cached`. A clean CI checkout of a PR has no working-tree/index changes, so committed PR changes are invisible. The command is documented as a CI gate for a pending diff, but has no base-ref/range input.

**Fix:** Add explicit base/head comparison for the PR gate, resolve the merge base, and supply those refs from CI. Preserve an explicitly selected working-tree mode for local use.

### F10 · P1 · Risk failures are silently omitted from the gate

**Evidence:** [catch around riskReport](/C:/Nodesify/nodesify-graphify/packages/astria-cli/src/commands/merge-gate.ts:124).

Any risk error is treated like an intentional outside-Git skip. Database failures and failed Git execution remove the check from `checks`, and `every()` can still report success.

**Fix:** Detect a non-Git directory explicitly and report a visible skip there. When a Git-backed gate cannot calculate risk, record a failed check with its cause.

## Graph identity, persistence, and integration

### F11 · P1 · Symbol/file identifiers are not collision-free

**Evidence:** [file and symbol naming](/C:/Nodesify/nodesify-graphify/crates/astria-extract/src/naming.rs:10), [normalization](/C:/Nodesify/nodesify-graphify/crates/astria-core/src/ids.rs:18), [duplicate suppression](/C:/Nodesify/nodesify-graphify/crates/astria-build/src/lib.rs:87).

File stems drop extensions, flatten path components with underscores, and are lowercased with punctuation normalization. `src/foo.ts` and `src/foo.js`, `a/b_c.rs` and `a_b/c.rs`, and case-distinct symbols can share IDs. The builder skips duplicate IDs, losing one definition and combining relationships.

**Fix:** Separate case-sensitive structural identity from normalized search text. Encode complete relative paths, extensions, lexical scope, and any declaration disambiguator in collision-resistant stable IDs; do not add a legacy-ID fallback.

### F12 · P1 · Global replacement deletes old data outside its transaction

**Evidence:** [global_add](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/global.rs:127), [prune_tag](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/global.rs:300).

Existing tag nodes/edges are pruned before the insertion transaction starts. If reading/inserting the replacement fails, rollback cannot restore the previously committed graph. Pruning itself uses separate statements.

**Fix:** Perform prune, replacement, and required relation reconciliation in one transaction. A failed replacement must retain the prior complete tag graph.

### F13 · P1 · Automatic global tags can duplicate a repo or replace another

**Evidence:** [pick_tag](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/global.rs:48).

The allocator knows taken tags but not which root owns them. Re-adding the same root without `--as` chooses a new tag instead of updating its old one. It also checks raw names against stored normalized names, then normalizes only afterward; names such as `Foo-Bar` and `foo_bar` can converge onto an existing tag and trigger its pruning.

**Fix:** Persist a canonical root-to-tag identity, normalize before checking collisions, and reject ownership conflicts. Repeat additions should update the same root's registration.

### F14 · P1 · Merge silently conflates same IDs from different repositories

**Evidence:** [merge_graphs](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/merge.rs:22), [second graph node suppression](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/merge.rs:132).

IDs intentionally use repository-relative naming. Two unrelated roots with `src/index.ts::main` therefore share an ID. Merge keeps the first node and attaches edges from both graphs to it. This is unsafe for cross-repository merge, and conflicting versions are silently first-wins for same-repository inputs too.

**Fix:** Define merge identity explicitly: namespace different repository identities and remap all endpoints; for the same repository, expose a conflict policy rather than silently dropping a definition.

### F15 · P2 · Merge accumulates edges and has no atomic publication

**Evidence:** [merge destination opening](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/merge.rs:31), [edge insertion](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/merge.rs:219).

Merge opens an existing output database and appends every edge. The schema has no uniqueness constraint on logical edges. Repeating a merge increases duplicate edges and distorts degrees/clusters; a failure leaves a partial destination.

**Fix:** Construct an explicit destination snapshot transactionally, deduplicate with the project's defined evidence identity, and make repeated identical merges yield the same graph. Reject output/input aliasing if in-place semantics are unsupported.

### F16 · P2 · Merge drops metadata and produces an incomplete artifact directory

**Evidence:** [node copy projection](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/merge.rs:96), [edge copy projection](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/merge.rs:190), [merge report write](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/merge.rs:44).

Copied nodes omit signatures, copied edges omit context, and merge writes only a report after clustering. It does not generate `graph.json` or publication provenance. `status` explicitly treats a database without `graph.json` as incomplete. Lost edge context also removes derived-pass ownership information.

**Fix:** Share a current-schema graph publication contract for merge and pipeline outputs. Preserve metadata meaningful to merged graphs, generate the advertised artifacts, and stamp provenance according to what the merge actually represents.

### F17 · P1 · Docker build omits required npm manifests

**Evidence:** [Dockerfile](/C:/Nodesify/nodesify-graphify/Dockerfile:35).

The builder copies Cargo manifests, crates, packages, and scripts, but not the root `package.json` or `package-lock.json`. It later runs `npm ci` from `/build` and a workspace build. The root lockfile/workspace metadata is unavailable to those commands.

**Fix:** Copy the root npm manifests before installation and use the intended workspace lockfile consistently. Ensure the Docker context includes every required workspace manifest.

### F18 · P2 · Viewer build writes to an unused asset location

**Evidence:** [viewer build script](/C:/Nodesify/nodesify-graphify/packages/viewer/package.json:8), [embedded bundle](/C:/Nodesify/nodesify-graphify/crates/astria-export/src/export_html.rs:11).

The viewer build writes `crates/astria-napi/src/assets/viewer.js`, while the exporter embeds `crates/astria-export/assets/viewer.js`. Building changed viewer source therefore does not update the shipped viewer. Main CI builds the CLI/native module without verifying this bundle's correspondence to source.

**Fix:** Correct the output destination and include viewer bundle generation or a source/bundle drift check in the release build.

## Ingestion and semantic enrichment

### F19 · P1 · Minified-text filtering is applied to binary documents/media

**Evidence:** [discovery filtering](/C:/Nodesify/nodesify-graphify/crates/astria-detect/src/lib.rs:157), [heuristic](/C:/Nodesify/nodesify-graphify/crates/astria-core/src/security.rs:192).

The newline-density heuristic runs for every classified file, including binary files. For example, a sufficiently large WAV with a mostly zero-valued sampled prefix has no newlines and is classified as minified, then silently excluded before transcription. Binary format and prose layout are not reliable indicators of generated source.

**Fix:** Apply the heuristic only to relevant textual source formats. Do not run it on media, Office documents, PDFs, or images; narrow prose filtering separately if needed.

### F20 · P1 · Semantic enrichment reads supported binary inputs as UTF-8

**Evidence:** [Office/media extraction routes](/C:/Nodesify/nodesify-graphify/crates/astria-extract/src/engine.rs:98), [semantic raw reader](/C:/Nodesify/nodesify-graphify/crates/astria-semantic/src/lib.rs:227), [failure abort](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/semantic_pass.rs:265).

AST/document extraction converts Office/Workspace/media content, but semantic enrichment independently reads original files as UTF-8, except PDFs and images. DOCX/XLSX and successfully transcribed audio/video can then fail enrichment and abort core publication. Workspace shortcuts enrich the link wrapper rather than exported document content.

**Fix:** Pass the same normalized extracted content to semantic enrichment that the document layer uses. Retain a separate image route and do not reinterpret binary files as source text.

### F21 · P2 · Skipped external ingestion is not retried on unchanged runs

**Evidence:** [uncached unavailable extraction](/C:/Nodesify/nodesify-graphify/crates/astria-extract/src/engine.rs:126), [media skip behavior](/C:/Nodesify/nodesify-graphify/crates/astria-extract/src/engine.rs:149), [needs_build condition](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/pipeline.rs:676), [manifest publication](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/graph_update.rs:115).

Missing media tools/models or Workspace credentials produce empty uncached extractions, while the manifest still advances. A later unchanged pipeline run skips extraction altogether, so installing the dependency does not trigger the promised retry unless another build condition changes. Changed files with previously valid content can also lose that content during an unavailable run.

**Fix:** Track unavailable/failed extraction separately from successful file freshness. Include pending retries in build decisions, and preserve the last usable extraction until replacement succeeds.

### F22 · P2 · Workspace documents are cached by shortcut bytes only

**Evidence:** [file-hash cache](/C:/Nodesify/nodesify-graphify/crates/astria-extract/src/engine.rs:51), [Workspace cache hit](/C:/Nodesify/nodesify-graphify/crates/astria-extract/src/engine.rs:113).

Cloud document edits normally leave local `.gdoc`/`.gsheet`/`.gslides` shortcut bytes unchanged. Cached extraction therefore never notices remote edits, even when users request an ordinary update.

**Fix:** Use remote revision/modified metadata in the source fingerprint and offer an explicit bounded refresh behavior. Define how remote freshness interacts with offline mode instead of claiming local shortcut hashes represent remote content.

### F23 · P2 · Invalid LLM replies become successful cached empty results

**Evidence:** [reply parser](/C:/Nodesify/nodesify-graphify/crates/astria-semantic/src/prompt.rs:41), [OpenAI extraction](/C:/Nodesify/nodesify-graphify/crates/astria-semantic/src/backend_openai.rs:165), [successful semantic cache](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/semantic_pass.rs:242).

Missing reply text, malformed JSON, refusals, or truncated output become `SemanticExtraction::empty()` and are returned as success. The semantic cache then prevents an unchanged retry. Valid intentional empty output and failed parsing are indistinguishable.

**Fix:** Return an explicit parsing/response failure for unusable replies and cache only validated successful responses, including intentional schema-valid empties. Check provider finish/refusal status where available.

### F24 · P2 · The advertised hard LLM budget is only a post-response threshold

**Evidence:** [budget check](/C:/Nodesify/nodesify-graphify/crates/astria-semantic/src/enrichment.rs:94), [file-level check](/C:/Nodesify/nodesify-graphify/crates/astria-semantic/src/lib.rs:199), [chunk loop](/C:/Nodesify/nodesify-graphify/crates/astria-semantic/src/chunking.rs:66).

Workers check recorded usage before starting a file. Multiple workers can pass at once; a file can issue several chunk calls without another budget check. No input/output reservation is made, so configured spend can be exceeded by in-flight responses and subsequent chunks.

**Fix:** Check and reserve a conservative per-request allowance atomically, apply it to every backend/judge/chunk call, and constrain output to the remaining allowance. If exact provider billing cannot be guaranteed, disclose the bounded estimate rather than a hard ceiling.

### F25 · P2 · Semantic HTTP retry/status handling does not match ureq behavior

**Evidence:** [post_json](/C:/Nodesify/nodesify-graphify/crates/astria-semantic/src/http.rs:27), [agent configuration](/C:/Nodesify/nodesify-graphify/crates/astria-semantic/src/http.rs:73).

The agent retains ureq's default `http_status_as_error = true` (verified in the installed ureq 3.3.0 source). Thus HTTP 4xx/5xx take the generic error branch; the success branch's Retry-After parsing and non-retryable 4xx handling do not run. That retries authentication/validation failures and discards useful API errors. Independently, its status branch computes a 500-based millisecond backoff and then multiplies by 1,000; enabling that branch unchanged would cause 500/1,000/2,000-second waits without Retry-After.

**Fix:** Handle statuses in one reachable branch using the library's configured behavior, retry only retryable failures, retain bounded diagnostic bodies, and use clearly typed duration units.

### F26 · P2 · Semantic chunking silently truncates content and does not cap long lines

**Evidence:** [split_chunks](/C:/Nodesify/nodesify-graphify/crates/astria-semantic/src/chunking.rs:15), [eight-chunk limit](/C:/Nodesify/nodesify-graphify/crates/astria-semantic/src/chunking.rs:66).

A single line larger than `MAX_CHUNK_CHARS` is never split. Inputs needing more than eight chunks silently discard the tail, but the resulting extraction is treated as complete and cached. Both can misrepresent coverage; long lines can exceed intended request size.

**Fix:** Split oversized lines at Unicode-safe boundaries. Return explicit partial-coverage/truncation metadata or process all allowed content within an explicitly reported budget policy.

### F27 · P2 · JSON export loses evidence provenance

**Evidence:** [edge export projection](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/pipeline.rs:889).

The current edge schema contains `source_line` and `context`, but `graph.json` exports neither. File consumers and JSON merges cannot preserve the line anchor or derived-pass ownership available in SQLite.

**Fix:** Export the current evidence fields consistently and document which additional database state is intentionally outside the graph exchange format.

### F28 · P2 · Exported artifacts can be torn or represent mixed snapshots

**Evidence:** [report and graph writes](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/pipeline.rs:821), [JSON exporter](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/pipeline.rs:843), [HTML payload reads](/C:/Nodesify/nodesify-graphify/crates/astria-export/src/export_html.rs:165).

JSON/HTML exporters read multiple tables without a shared read transaction, unlike query snapshots. A concurrent updater can publish between reads. Report and JSON files are written directly to final paths, allowing a concurrent reader or interrupted process to see truncated/inconsistent files. Core SQLite publication is atomic, but derived graph/artifact publication has a separate consistency boundary.

**Fix:** Capture each export in one read snapshot; write complete temporary files and atomically replace published artifacts. Stamp a common generation so consumers can recognize mismatched report/JSON/database versions.

### F29 · P2 · Non-ASCII document titles can panic during filename creation

**Evidence:** [derive_doc_name](/C:/Nodesify/nodesify-graphify/crates/astria-ingest/src/lib.rs:307).

The slug retains Unicode alphanumeric characters, then calls `String::truncate(80)`, which requires a UTF-8 character boundary. A title of 27 Chinese characters is 81 bytes; truncation at byte 80 splits a character and panics.

**Fix:** Limit by characters, or find a valid boundary under the byte cap. Apply the existing Windows-reserved filename treatment to generated document names too.

### F30 · P2 · URL classification uses the whole URL instead of parsed host/path

**Evidence:** [classify_url](/C:/Nodesify/nodesify-graphify/crates/astria-ingest/src/lib.rs:45).

Substring host checks can classify `https://example.com/x.com/...` as a tweet, and extension checks fail for `file.pdf?download=1` or `audio.mp3?token=...`. Those downloads then take text/webpage routes, damaging saved content and selecting the wrong integrations.

**Fix:** Parse once with the existing `url` dependency, match canonical hostnames and path extensions, and use response content type where classification needs refinement.

### F31 · P2 · Office compressed-size limits do not bound decompressed content

**Evidence:** [DOCX ZIP part reading](/C:/Nodesify/nodesify-graphify/crates/astria-office/src/lib.rs:252), [XLSX range loading](/C:/Nodesify/nodesify-graphify/crates/astria-office/src/lib.rs:197).

DOCX reads an archive member into an unbounded String. XLSX loads worksheet ranges before applying output row/column caps. The discovery cap applies to compressed file bytes, so a small highly compressed input can allocate much larger content.

**Fix:** Enforce expanded byte/entry limits before and during ZIP reads, and bound worksheet expansion using the library's supported APIs. Return explicit size-limit failures.

### F32 · P2 · Watch mode misses supported file types and directory renames

**Evidence:** [watch filters](/C:/Nodesify/nodesify-graphify/packages/astria-cli/src/commands/watch.ts:6), [rebuild callback](/C:/Nodesify/nodesify-graphify/packages/astria-cli/src/commands/watch.ts:36).

Watch listens to a small language subset, excluding Ruby, PHP, Kotlin, Markdown, manifests, ignore files, and many other ingested formats. Directory rename events with no extension are dropped, leaving old paths until a separate accepted event occurs. The synchronous native pipeline also blocks the event loop throughout rebuilding, delaying event/signal handling.

**Fix:** Share discovery's supported-input/ignore policy and reconcile directory events. Run rebuild work outside the main event loop with one bounded queue. Add watcher error handling and validate debounce as a finite positive number.

### F33 · P2 · Database decoding errors silently produce incomplete graphs

**Evidence:** [query node loading](/C:/Nodesify/nodesify-graphify/crates/astria-query/src/store.rs:109), [merge row loading](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/merge.rs:119), [JSON export row loading](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/pipeline.rs:873).

Many authoritative graph reads use `filter_map(|r| r.ok())`. Corruption, type mismatches, or row-read errors drop records instead of failing. Queries/merges/exports then claim successful results without indicating omitted nodes or edges.

**Fix:** Collect authoritative rows as `Result<Vec<_>, _>` and propagate errors. Limit best-effort dropping to clearly optional diagnostics that disclose omissions.

### F34 · P2 · Viewer navigation is incomplete for keyboard and screen-reader users

**Evidence:** [search result rows](/C:/Nodesify/nodesify-graphify/packages/viewer/src/viewer.ts:754), [search keyboard handling](/C:/Nodesify/nodesify-graphify/packages/viewer/src/viewer.ts:783), [global keyboard handling](/C:/Nodesify/nodesify-graphify/packages/viewer/src/viewer.ts:1000).

Search results are clickable divs under a listbox without option semantics, focusability, or selection keyboard controls. Enter selects only the first match; graph neighborhood inspection remains pointer-driven. The canvas summary and zoom shortcuts provide a useful baseline, but do not provide equivalent navigation through the results/relationships.

**Fix:** Implement a navigable result list with appropriate roles, active selection and focus management; expose selected-node relationships in a keyboard-operable textual view.

### F35 · P1 · Bolt handshake interprets version bytes incorrectly

**Evidence:** [handshake](/C:/Nodesify/nodesify-graphify/crates/astria-bolt/src/lib.rs:117).

Bolt encodes minor then major in the final two bytes. The supposed 4.1 offer `0x00000401` encodes 1.4; the client then reads `picked[2]` as major, rejecting valid replies such as 3.0 or 4.1. Symmetric 4.4 hides this mistake. [Official Bolt handshake specification](https://neo4j.com/docs/bolt/current/bolt/handshake/).

**Fix:** Encode/decode versions according to the specification, retain the negotiated version, and reject versions the implementation cannot actually speak.

### F36 · P1 · Bolt PULL encoding does not match negotiated protocol versions

**Evidence:** [run/PULL encoding](/C:/Nodesify/nodesify-graphify/crates/astria-bolt/src/lib.rs:159).

The client sends PULL with an integer field `-1`. Bolt 4+ requires an extra dictionary containing `n`; Bolt 3 uses fieldless PULL_ALL. The one-size message is valid for neither advertised shape, preventing live pushes against conforming servers. [Official Bolt message specification](https://neo4j.com/docs/bolt/current/bolt/message/).

**Fix:** Use correct messages for the explicitly supported negotiated protocol. Prefer replacing this partial protocol implementation with a maintained driver if it meets the project's build requirements.

## Engineering improvements to schedule separately

These are recommendations supported by implementation structure, not additional claims of proven runtime failures.

### R01 · P2 · Restore TypeScript checking across the native interface

[native.ts](/C:/Nodesify/nodesify-graphify/packages/astria-cli/src/native.ts:135) exposes `(...args: any[])` and an `any` binding despite [generated declarations](/C:/Nodesify/nodesify-graphify/packages/astria-cli/astria.node.d.ts). Use the generated API type for the binding and wrappers so argument order, optionality, results, and native surface changes are checked during compilation.

### R02 · P2 · Measure and reduce per-request whole-graph work

[store.rs](/C:/Nodesify/nodesify-graphify/crates/astria-query/src/store.rs:66) loads all nodes/edges on each query. This provides freshness but incurs O(V+E) allocations per request; the HTTP transport multiplies those costs under concurrency. Establish latency/memory targets on representative large graphs, then share immutable snapshots keyed by the published generation or retrieve only needed graph slices. Preserve transaction-consistent reads.

### R03 · P2 · Make release dependency resolution reproducible

[ci.yml](/C:/Nodesify/nodesify-graphify/.github/workflows/ci.yml:73) and release/quality workflows use `npm install` rather than frozen lockfile installation. The comment explains nested installation and the napi version check, but that only checks one dependency, not the full resolved dependency tree. Use a lockfile-preserving installation with the required strategy; keep drift checks if still useful. Dependency update automation currently covers GitHub Actions only in [dependabot.yml](/C:/Nodesify/nodesify-graphify/.github/dependabot.yml:1); add planned npm/Cargo updates with the existing audit/build gates.

### R04 · P2 · Separate production and development documentation deployment

[docs.yml](/C:/Nodesify/nodesify-graphify/.github/workflows/docs.yml:7) deploys both `main` and `develop` to the same Pages destination and concurrency group. If develop is intended as preview work, it can replace public documentation before release. Reserve production deployment for the chosen release/main source and give development an explicit preview destination or build-only validation.

### R05 · P3 · Remove obsolete compatibility paths according to project policy

[env_var](/C:/Nodesify/nodesify-graphify/crates/astria-core/src/lib.rs:30), [migration command](/C:/Nodesify/nodesify-graphify/packages/astria-cli/src/commands/migrate.ts:1), and [merge schema fallback](/C:/Nodesify/nodesify-graphify/crates/astria-napi/src/merge.rs:183) still support old Graphify/environment/schema paths. The supplied instructions explicitly reject backward compatibility, fallbacks, and migrations. Decide the supported current format, remove obsolete branches and command/documentation references, and use a clear rebuild/error path. This is policy alignment; existing README promises must be updated with the implementation.

### R06 · P3 · Bring architecture and support counts up to date

[ARCHITECTURE.md](/C:/Nodesify/nodesify-graphify/ARCHITECTURE.md:7) and [README.md](/C:/Nodesify/nodesify-graphify/README.md:107) still describe 16 crates, while [Cargo.toml](/C:/Nodesify/nodesify-graphify/Cargo.toml:3) lists 20. Architecture's 25-language list also lags newly registered grammar modules. Generate support/count summaries from current manifests/registration instead of maintaining independent numbers. Keep the useful existing schema/language drift tooling and extend it to these overview claims.

## Suggested implementation order

1. **Trust and secrets:** F01–F07; fix TLS and protocol correctness together with F35–F36.
2. **Reliable decisions and identity:** F08–F14. Risk and merge output must be trustworthy before being used as release/merge gates.
3. **Publication and shipping:** F15–F18, F27–F28, F33.
4. **Working ingestion:** F19–F26, F29–F32.
5. **Usability and maintenance:** F34 and R01–R06.

Use current schemas and shared content/path/publication contracts to resolve repeated root causes. Keep changes bounded and modular, and remove obsolete paths rather than introducing compatibility layers.

## Limits of this review

No live cloud accounts, PostgreSQL instance, Neo4j server, remote MCP deployment, external LLM billing, published npm platform installations, or Docker build were exercised. Security impacts are source/specification-backed, with deployment prerequisites described above. No dependency vulnerability claims are made from version numbers alone; existing Rust/npm audit workflows were inspected but not run here. Runtime latency, benchmark validity across external corpora, and complete visual/mobile accessibility remain follow-up evaluation areas. The existing knowledge graph was not rebuilt as part of the review.

Website compilation result: **passed**. Docusaurus generated the production site and LLM-friendly documentation successfully.
