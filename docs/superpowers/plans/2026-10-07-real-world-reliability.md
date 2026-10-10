# Astria real-world reliability implementation plan

> Execute the approved review in the shared checkout. Repository instructions override skill defaults: no worktrees, no tests, no commits without a request. Use independent implementation ownership and review the integrated changes.

**Goal:** Make source navigation, change review, indexing, and retained knowledge dependable in everyday use.

**Architecture:** Preserve the Rust/tree-sitter/SQLite pipeline. Repair extraction evidence at its source, return complete bounded subgraphs, compare changed declarations using Git source without changing the checkout, serialize graph writers, and make indexing policy explicit. Retain document lifecycle metadata in extracted context and add a reproducible task-evaluation contract and reporting workflow.

**Tech stack:** Rust, tree-sitter, SQLite, Node.js/TypeScript, existing evaluation scripts; maintained protocol and locking libraries where required.

## Extraction and compiler evidence
- [x] Preserve receiver qualification and avoid binding unknown receiver methods to unrelated bare definitions.
- [x] Normalize all AST source locations to one-based lines and bump extraction cache schema.
- [x] Replace simplified SCIP ingestion with standard index documents, global symbol identities, occurrence locations and relationships, supporting native protobuf.

## Query answers and retained knowledge
- [x] Separate node discovery from complete qualifying edge collection, with deterministic deduplication.
- [x] Exclude unlocated stubs/references from file ranking and enforce actual tokenizer budgets.
- [x] Cache tokenized labels/documentation/IDs and corpus IDF in generation-bound graph snapshots.
- [x] Carry explicit current/resolved/superseded status and replacement links into document chunks and rendered results; annotate the historical review's resolved findings.

## Change review
- [x] Associate diff hunks with declarations rather than every symbol in a changed file.
- [x] Include base-source evidence for removed declarations/files and renames without worktrees or altering Git state.
- [x] Return direct/inferred impact, coverage gaps, commit identity, source locations, relevant tests and owners in a review brief.
- [x] Reuse the same impact engine in risk, PR triage and merge gates; surface failed PR-file retrieval.
- [x] Record imported repository commit/generation and report stale cross-repository snapshots.

## Indexing reliability
- [x] Hold an OS-backed per-project writer lock from discovery to artifact publication for all graph mutators.
- [x] Use unique sibling temporary files for artifact publication.
- [x] Reconcile watch mode on startup, bound debounce delay, recover spawn failures and shut down its own active child deliberately.
- [x] Persist a non-secret indexing profile and follow it during ordinary/watch/hook updates, with explicit paid-refresh budgets.
- [x] Count measured LLM usage from failed as well as completed runs.

## Evaluation and delivery
- [x] Add a runnable task-level paired evaluation/reporting workflow with pinned input provenance, correctness outcomes, elapsed time, token usage, source reads/wrong-file edits, and build/update amortization.
- [x] Preserve untouched reserved retrieval questions; do not fabricate measurements or invoke paid agents.
- [x] Update reference documentation and the implementation record.
- [x] Compile the Rust workspace and TypeScript CLI; refresh the graph using the newly built CLI in structural mode.
- [x] Review the integrated diff for scope, correctness, conflicts and unsupported claims.
