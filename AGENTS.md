

## astria

This project has an astria knowledge graph at `.astria/`. Access it through whichever path your agent has:

- MCP (ZCode and agents with the `astria` server connected): tools are `repo_map`, `query_graph`, `explain`, `get_neighbors`, `shortest_path`, `affected`.
- CLI (works everywhere, e.g. Codex): `astria map|query|explain|path|affected <args>`.

- Prefer the graph over repeated text searches when investigating architecture, locating a feature across files, tracing cross-file logic flow, or assessing change impact. Orient with `repo_map` (or `map`, or a skim of `.astria/graph_report.md`); ask natural-language questions with `query`; inspect one symbol with `explain`; run `affected <node>` before changing a shared symbol.
- Plain Grep/Read remains right for exact text, implementation details, and verifying graph results against source.
- Do not rebuild the graph on session start or before every command. After substantial edits, refresh it with `astria update .`; if results look stale or incomplete, verify against source files and update as needed.
- `/astria` loads the full usage skill.
