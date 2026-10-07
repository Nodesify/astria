# Installation lifecycle implementation plan

**Goal:** Implement the approved installation, cleanup, packaging, diagnostics and recovery fixes.

**Architecture:** Explicit project/user scopes isolate integrations. A scoped installation record retains shared registrations until their last consumer is removed. Whole-operation locking and atomic replacement protect configuration; errors remain failures.

**Tech stack:** TypeScript, Node filesystem APIs, existing native capability/provenance APIs and GitHub Actions.

User authorization covers implementation in the shared checkout. No worktrees, commits or new test cases. Verify compilation, documentation consistency, and disposable package smoke checks.

- [x] Protect read/modify/write operations with one user installation lock; uniquely stage durable file replacements and abort on read errors.
- [x] Implement scoped ownership records, retain shared platform resources, remove Pi extensions, report cleanup failures, separate graph/global purge.
- [x] Add doctor covering runtime, executable resolution, config ownership, hooks, writable targets, graph compatibility and embedding capability.
- [x] Align supported platforms and native load diagnostics; install packed artifacts in clean release runners and smoke-check real graph operations before publishing.
- [x] Document setup, scope changes, upgrades, restart requirements, backups, restore and complete package removal.
- [x] Compile CLI, run appropriate verification, refresh AST graph and record results.

Verification: CLI compilation and website production build passed; 578 existing CLI assertions passed. Windows x64 packed install passed with DirectML included. Disposable-project checks verified shared registrations, Pi cleanup, customized-file protection and scoped purge. Doctor reports no errors locally; release YAML parses and all seven runtime targets gate publishing. The cross-platform GitHub jobs have not been dispatched locally. Future releases require trusted-publisher bindings for the two new target packages and a new immutable npm version. Graph refresh uses the saved AST-only profile.
