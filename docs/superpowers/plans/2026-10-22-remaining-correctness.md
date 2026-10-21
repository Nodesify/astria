# Remaining correctness backlog implementation plan

**Goal:** Complete the remaining review backlog in TODO.md without committing TODO.md.

**Architecture:** Keep resolution conservative and provenance explicit. Use the
existing Rust extraction/query pipeline and CLI; preserve distinct relationships
through all presentation paths. Reuse the shared language registry and security
helpers rather than introducing parallel configuration systems.

**Tech stack:** Rust, tree-sitter, SQLite, TypeScript CLI/viewer.

User constraints override generic skill defaults: shared checkout, no worktrees,
no test creation or execution, compile/build verification only, no automatic
commits. Use subagent-driven-development with separate file ownership.

- [x] Query/hooks: ORIG_HEAD merge refresh with complete input coverage; ambiguous
  seed candidates through shared CLI/MCP result rendering; audit and fix parallel
  relationships in query/export/viewer.
- [x] Language coverage: bounded shebang discovery shared with parser/watch/cache;
  Kotlin aliases, C# delegates, Swift macros/calls, Dart typedef declarations using
  installed grammar metadata; invalidate changed extraction output.
- [x] Security/diagnostics: shared secret redaction before persistence/truncation
  and semantic requests; expose semantic processed/total chunks and failure reasons
  without weakening hard failure for partial enrichment.
- [x] Typed receivers/identity: infer fields assigned in methods only when type
  evidence is unambiguous; respect declarations and unknown/conflicting assignments;
  audit punctuation-sensitive identity and public/private call matching.
- [x] Review each implementation against TODO requirements and inspect quality;
  compile affected Rust crates and build changed TypeScript packages; resolve
  integration failures, refresh Astria graph, update TODO status with evidence.
