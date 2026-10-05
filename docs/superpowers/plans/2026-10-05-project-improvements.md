# Project trust and usability implementation plan

**Goal:** Implement all five recommendations from the project evaluation and simplify the README.

**Architecture:** Extend the existing extraction, ranking, clustering, reporting and evaluation modules. Keep name-derived bindings explicitly RESOLVED, preserve unknown targets, and propagate authoritative SQLite decoding failures. Use deterministic source metadata for meaningful reports and a reproducible, separately authored evaluation corpus with an iterative search baseline.

**Tech stack:** Rust, rusqlite, tree-sitter, TypeScript, existing Node benchmark tooling.

User authorization: “impl all fix” approves the preceding review. Work in the shared checkout; preserve unrelated edits. No new tests or test execution. Verify with compilation, type checks, script syntax checks and graph refresh. Do not publish, commit unrelated work, or invent benchmark results. If another writer changes a file during editing, skip the conflicting edit and flag it.

## Task 1: Definition retrieval and reference resolution

- [x] Inspect `crates/astria-extract/src/refs.rs`, extraction metadata and `crates/astria-query/src/scoring.rs`; run graph affected checks.
- [x] Resolve qualified names against matching scope suffixes before bare names. Use existing import relationships for file narrowing where a target can be identified unambiguously; never guess across colliding definitions or unsupported alias forms. Preserve evidence distinctions and invalidate extraction caches only if necessary.
- [x] Prefer exact qualified implementation matches over prose mentions without forcing code intent on document-only questions. Keep matching deterministic and ambiguous alternatives discoverable.
- [x] Compile changed crates; report behavior and remaining language limitations without claiming compiler-grade resolution.

## Task 2: Actionable reports and authoritative error propagation

- [x] Inspect current edits to `crates/astria-analyze/src/lib.rs`, `crates/astria-report/src/lib.rs` and clustering; preserve them.
- [x] Replace discarded authoritative row errors with Result collection/propagation, including persisted community enrichment. Keep genuinely optional missing metadata optional.
- [x] Improve default community names using source module/package paths; avoid closures and generic hub names. Preserve valid LLM labels.
- [x] Separate production source orientation from documentation and test/benchmark orientation in reports. Link hubs and interesting relationships to source and expose relationship evidence rather than presenting all edges equivalently.
- [x] Compile affected crates.

## Task 3: Independent evaluation and documentation

- [x] Inspect existing paired/external baseline runners and dataset contracts.
- [x] Add untouched reserved questions grounded in pinned external repositories beyond the existing tiny corpus; record authoring provenance and mark any source-inspected cases honestly. Do not run evaluation because the user forbids tests.
- [x] Implement a bounded deterministic iterative rg-plus-source-read baseline using existing utilities, with no golden answers available to its search. Record search rounds, read cost, failures, file and definition ranking and delivered context cost alongside graph build cost.
- [x] Document invocation, limitations, reserved-set protection and fair comparison. Do not claim general superiority or fabricate measured results.
- [x] Correct snapshot-cache and EXTRACTED/RESOLVED contradictions in root and website architecture docs; extend documentation checks to these contracts where practical.
- [x] Simplify README around build → locate implementation → inspect relationships → assess change, moving detailed integrations to existing guides.
- [x] Syntax-check scripts and compile/type-check the applicable workspaces.

## Completion

- [x] Review spec coverage and code quality for all tasks; resolve actionable findings.
- [x] Run `cargo check --workspace --locked`, CLI and viewer type checks, and relevant documentation/build checks without test execution.
- [x] Refresh the graph after final modifications and disclose if the installed native binary predates changed source.
- [x] Summarize implemented outcomes, validation, and any remaining reserved evaluation or external-service limitations.

Validation: Rust workspace check, CLI build, viewer type check, website build, script syntax and documentation drift checks passed. Review findings were corrected. Reserved evaluation remains unrun under the project policy. The graph refresh uses the freshly built native module directly because a running process locks the installed module; its replacement is staged as astria.node.new. Work remains uncommitted in the shared checkout.
