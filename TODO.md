# TODO

## Roadmap (see [PLAN.md](PLAN.md) for the full implementation plan)

- [x] **Phase 1 — Ecosystem plumbing** (2026-09-25): validation in build, `diagnose` command, JSONL query log + orientation stamp, `save-result` + `reflect`, always-on instruction blocks. `hook-guard` command shipped; strict-mode wiring into .claude/settings.json pending on the install side.
- [x] **Phase 2 — Hypergraph** (2026-09-25): `hyperedges` table (schema v7), deterministic producers (community `participate_in` + shared-reference `shares_reference`), graph.json/report/wiki/HTML-hull/explain consumers.
- [x] **Phase 3 — Ingest breadth** (2026-09-25): cargo path-dep + workspace-inherited topology, .mcp.json ingest (env names only), SCIP JSON ingest (`add --scip`), Postgres introspection via psql (`add --postgres`), transcript sidecar contract (`.graphify/transcripts/*.txt|md`).
- [x] **Phase 4 — Cross-repo global graph** (2026-09-25): `~/.nodesify-graphify/global.db` store, repo-tag prefixed merge with external unification by label, re-deriving `same_type_as` + cross-repo call passes (fail closed on ambiguity), `global add/remove/list/path`, `run --global --as`, `query/explain/path --graph <db>`. Naive `merge.rs` left as-is (superseded by global).
