---
status: current
---

# Astria reliability implementation — 7 October 2026

The approved real-world review has been implemented in the shared checkout. Changes are uncommitted. This record supersedes the historical October 3 defect backlog for current implementation guidance.

## Source evidence and query answers

Unknown receiver calls retain qualified, caller-scoped identities and remain inferred rather than binding to an unrelated bare method. Explicit receivers and lexical scope provide resolution evidence; syntax extraction still does not perform general type inference. AST citations use one-based lines, and extraction fingerprints advance to v16 so old cached locations and resolutions are rebuilt.

Node discovery and relationship collection are separate. Queries include qualifying edges between selected seeds, cycles and multiple relationships. Repo maps exclude unlocated stubs/references and use actual response-token counts for complete records, including empty-response validation. Generation-bound snapshots cache lexical fields and corpus document frequencies; query components are tokenized once per request.

Markdown frontmatter supports `status: current|resolved|superseded`, `resolution`, and `superseded_by`. Document nodes, sections, chunks and their semantic concepts inherit the lifecycle metadata. Answers disclose it; resolved/superseded material receives lower lexical relevance unless history is requested. Historical information remains accessible. The October 3 review is explicitly marked resolved.

## Change review

`risk`, PR triage and merge gates share one source-based engine. Git objects and working/index bytes are extracted in memory without changing the checkout. Diff hunks select enclosing declarations; before-source evidence retains deleted declarations and their consumers. Reports include immutable commit/source identities, evidence chains and confidence, test consumers, CODEOWNERS matches, coverage issues, and source-indexing cost. Coverage gaps produce an unknown score and fail the gate instead of becoming zero risk.

PR triage uses the actual PR base/head objects. Missing GitHub data or local objects produces an explicit coverage error. Shared changed/affected symbols indicate review coordination, without claiming a proven merge conflict. Syntax-only inference remains incomplete for dynamic dispatch and unsupported structural grammars; it is evidence for review, not proof of runtime safety. Both revision corpora are indexed for a review, so large-repository review cost is visible rather than assumed free.

## Compiler and cross-repository evidence

SCIP ingestion accepts official protobuf indexes and protocol JSON, with validated project/document paths, global symbol identities, one-based occurrence citations and independent relationship flags. Unlocated external metadata has no invented source citation. Index owners retain their own symbol facts; canonical symbols select the best surviving definition. Reimport replaces the owner's facts. Changes to an indexed source invalidate the affected complete compiler overlay and `status` identifies indexes needing reimport.

Global repository imports record source commit, graph generation and build time. Listings and path answers disclose current/stale/unknown/unavailable state. Paths render relationship direction and confidence, and ambiguous labels require an exact ID. Inferred cross-repository relationships remain explicitly inferred.

## Indexing and operations

Project writers hold OS-backed locks across source discovery, mutation and publication. Import, memory, transcript, clustering, merge and global writes use their appropriate lock scope. Artifact replacement uses unique temporary siblings. Generation stamps identify each publication; separate artifacts can be compared for consistency.

The non-secret indexing profile pins effective routing, model, budget and enrichment options. Updates, watch, hooks and ingestion reuse it. Automatic updates without a profile select structural indexing. Changing paid policy requires explicit refresh; a paid replacement requires a positive token budget. Disabled embeddings remove their vectors and similarity edges instead of automatically running a cached local model. Update supports explicit enabling/disabling of saved feature flags. Watch reconciles at startup, bounds debounce, releases in-flight state after spawn errors, queues updates and terminates only its own child. Cost totals include measured usage from failed and completed runs.

## Evaluation and verification limits

[The task evaluation workflow](../scripts/bench/tasks/README.md) pairs baseline/Astria tasks at pinned source revisions with the same agent/settings, retains correctness review separately, and reports measured time, tokens, source reads, wrong-file edits and indexing amortization. Missing measurements remain unknown. The example manifest covers real bugs at the pre-fix commit. It requires supplied isolated disposable copies and an agent adapter; it does not create worktrees or alter this checkout.

Local builds and test suites were run at the user's request. The Rust workspace suite passed 585 tests with four ignored; CLI, installation, hook, merge-driver and end-to-end suites passed 574 tests. Rust workspace, CLI/viewer and website builds succeeded. Verification also corrected a repo-map test to check strict token budgeting and normalized documentation checks for Windows line endings. Paid task evaluations were not run. Reserved retrieval suites were left unchanged. These checks do not establish measured task-quality improvement or production runtime coverage.
