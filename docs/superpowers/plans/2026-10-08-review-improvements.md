# Review improvements implementation plan

**Goal:** Implement all six improvements approved in the project review.

**Architecture:** Keep retrieval, freshness and result contracts in Rust and reuse them through N-API and MCP. Retain explicit provenance and source-based correctness while avoiding repeated derived work when all effective inputs are unchanged.

**Verification:** Compile Rust and TypeScript, validate JavaScript syntax, inspect the final diff, and refresh the graph structurally. Per the user's project instructions, do not create or execute tests. Leave changes uncommitted in the shared checkout.

- [x] Prioritize located implementations in code queries; retain compact unresolved relationship evidence.
- [x] Add shared content-based source freshness with discovery of added files, generation/artifact checks, and separate age reporting.
- [x] Return typed query nodes, edges, pagination, freshness and generation through CLI JSON and MCP; validate MCP arguments.
- [x] Strengthen existing quality gates for definitions, query failures and token budgets using an explicitly recorded baseline.
- [x] Make downstream evaluation executable with a concrete adapter and external task preparation; report any missing runtime or credentials honestly.
- [x] Record indexing stage costs and memory, skip safe unchanged derived work, and reduce avoidable query candidate work.
- [x] Compile, review, and document outcomes.
- [ ] Refresh the graph structurally: blocked by SQLite corruption confirmed by a read-only quick_check.

The previously presented review is the approved design. Evaluation execution requires explicit supplied inputs and working agent access; an implementation cannot fabricate task success or a quality baseline.

Implementation: all six changes are complete. Rust workspace checks, native compilation, CLI TypeScript compilation, JavaScript syntax checks and independent source review are the verification used. No tests or evaluations were executed. Structural graph refresh was attempted with the freshly built native runtime but failed with SQLite database disk image is malformed. Read-only quick_check confirmed out-of-order row IDs, duplicate page references, and damaged query_pairs entries. Rebuilding the cached graph requires a separate recovery decision; the existing database was not replaced. Active Windows processes retain the previous native DLL; the latest build is at packages/astria-cli/dist/astria.node.new. See docs/review-improvements.md for the contracts and evaluation prerequisites.
