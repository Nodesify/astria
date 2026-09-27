# Retrieval correctness implementation plan

Approved by the user's “impl all” following the saved benchmark diagnosis.

**Goal:** Preserve real definitions, extract assigned JS/TS functions, improve identifier retrieval, enforce exact response budgets, and measure symbol-level correctness independently of file hits.

**Architecture:** Code definitions retain identity; semantic dedup cannot delete them. Shared extraction handles bound functions. Query scoring recognizes full identifiers and intent, and shared rendering owns token accounting. Frozen legacy questions and separately versioned independent cases prevent silently changing the baseline.

**Validation:** Build/check Rust and CLI; rerun explicitly authorized benchmarks and symbol diagnostics. No new unit/e2e test suites, no worktree, no commits unless requested.

- [x] Protect code identities in dedup; ensure updates restore lost definitions.
- [x] Extract assigned JS/TS functions with qualified bindings, scope, body and documentation; invalidate extraction cache. Prefer Python implementations over overload declarations.
- [x] Improve complete-identifier coverage and implementation-aware ranking without hiding requested tests/docs.
- [x] Enforce exact whole-response token budgets consistently for CLI and MCP.
- [x] Add reproducible paired benchmarking, symbol-preservation checks and independently versioned questions; retain the legacy baseline.
- [x] Review cross-component changes, compile, rerun benchmarks, refresh project graph and document measured outcomes and limitations.

## Measured validation — September 27, 2026

Saved local evidence: `bench-work/paired-verified-20260927/{results.json,comparison.md,query-contract.json,update-restoration.json}`. Baseline: `bench-work/paired-20260927-162610`. Candidate is the local release artifact (hash in results), based on 71e70ef with these uncommitted changes; Graphify is pinned to 4139885a1212956cf69a76946fbde0d181ab85e9. Corpus commits and golden hashes are recorded. No LLMs or embeddings; one observation per condition, process startup included. These small sets establish regressions and diagnostics, not general superiority.

| Frozen corpus | Questions | Previous Astria recall@5 | New Astria recall@5 | Graphify recall@5 |
|---|---:|---:|---:|---:|
| Self | 35 | 77.1% | 78.6% | 67.1% |
| Click | 3 | 66.7% | 100% | 100% |
| Express | 2 | 0% | 100% | 100% |
| ripgrep | 3 | 100% | 100% | 100% |

Above: 4,000 tokens, distinct-file recall, original goldens unchanged. Self MRR regressed 0.694→0.674 and hit@5 85.7%→82.9%, despite higher recall. The corrected self-v2 file is available separately and was not substituted into this comparison.

Seven additional authored validation questions are reported separately: Click 100% vs 50%, Express 100% vs 50%, ripgrep 66.7% vs 33.3% file recall@5. They have been used during validation and are not untouched held-out evidence. Exact-definition recall remains lower: Click diagnostic 60%, Click additional 0%, Express 100%, ripgrep additional 25%. Correct file retrieval does not establish correct symbol retrieval or answer correctness.

All 200 query processes succeeded. Astria had zero raw-budget overruns across 100 responses at 1,000/4,000 tokens. CLI/MCP text matched exactly at 993 tokens for a 1,000-token diagnostic; three 500-token pages advanced 0→14→24→34 with complete node/edge records and no repeated node IDs. Invalid or insufficient budgets report errors.

All cached scoped IDs survived: self 1345/1345, Click 1147/1147, Express 3263/3263, ripgrep 3076/3076. All seven known Click/Express diagnostic definitions were present at their grounded implementation lines. Updating a graph built with the previous binary restored 151 previously lost Click IDs (996/1147→1147/1147), with zero LLM calls. ID retention is not a completeness claim; the audit additionally reports ID/location collisions.

Tradeoffs: original-corpus query means are now 0.29–0.37 seconds versus 0.14–0.16 before, with exact token accounting and larger graphs; paired Graphify means are 0.62–1.05 seconds. New build seconds (Astria/Graphify): self 9.63/14.55, Click 4.83/7.14, Express 10.15/6.95, ripgrep 10.83/10.74. Express now extracts many more assigned functions and closures; Astria no longer wins every build timing. Measurements are single runs and noisy.

Release native build, workspace clippy with warnings denied, CLI build, website build, docs synchronization and diff whitespace checks passed. Existing test targets were compile-checked; no unit/e2e suites were run or new test cases added. The project graph was refreshed with the candidate native artifact.

Remaining limitations: runtime receiver/type aliases are not inferred; some multi-file and exact-symbol rankings remain weak. Future ranking work should use newly authored evaluation questions, inspect those misses, and profile tokenization/rendering before changing scoring again.
