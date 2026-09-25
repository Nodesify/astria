# TODO

## DONE — Port PHP closures from reference graphify v0.9.67 (v8 branch) — 2026-09-25

Implemented on `develop`:
- `langs/config.rs`: new `closure_types` field (empty default; opt-in per language).
- `langs/php.rs`: `closure_types = ["anonymous_function", "arrow_function"]` + 7 ported tests (route naming, ordinals, nested prefix composition, non-route false-positive guard, file/method-scope call attribution, no duplicates).
- `walkers.rs`: closure name synthesis (`VERB /path` via AST walk for routing calls incl. `group()`/`prefix()` prefixes; else stable per-scope ordinal), and closures as call-attribution boundaries — calls inside attribute to the closure, not the enclosing function.
- Ordinal labels are scope-qualified (`PriceCalc::{closure#1}()`, `routes_api::{closure#1}()`) — found via e2e test that the build's fuzzy label-dedup merges same-label nodes and would misattribute the second closure's call edges onto the first. Labels must stay corpus-unique.
- Verified end-to-end: CLI run + query + explain on a Slim-style routes fixture; 33/33 extract tests, workspace suite, clippy + rustfmt clean.

At next release, changelog note: call edges inside PHP closures now attribute to the closure node; existing graphs see those edges move on next `update`.

Known parity gap with upstream (accepted): fluent `Route::prefix('/x')->group(...)` prefixes are only composed for member-call receivers (Slim style), not scoped-call facades — same limitation as the reference implementation.

## Not doing (deliberate)

- **Hypergraph (reference main, d23ae17)** — consumer with no producer: reference generates hyperedges from an LLM step we replaced with local embeddings. Dead feature until we pick a deterministic producer; revisit only if a real need shows up.
- **5 new languages (COBOL, VB.NET, R, Solidity, Erlang, v0.9.66)** — one new tree-sitter grammar dep each, nobody asked. Add when a repo needs one.
- **JS/TS anonymous functions** — same gap exists there (`arrow_function`/`function_expression` dropped); the new `closure_types` field makes opting in a two-line change when someone asks.
