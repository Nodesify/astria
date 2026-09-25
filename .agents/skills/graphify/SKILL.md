---
name: graphify
description: Use nodesify-graphify for code architecture, feature location, cross-file logic flows, dependencies, and change-impact investigation in projects with an existing .graphify graph. Also use when the user invokes /graphify or explicitly asks to build or refresh a knowledge graph. Simple exact-text searches and edits to known files do not require this skill.
---

# Graphify

Use the project's knowledge graph to identify relevant code and relationships, then verify findings in source files. A graph is an index of extracted relationships, not proof of runtime behavior or complete reference coverage.

## Choose the workflow

- **Ordinary investigation:** Use an existing graph to answer the task. Do not enter the build workflow merely because this skill was selected or a hook mentioned Graphify.
- **Explicit `/graphify`, build, or refresh request:** Follow the build/update workflow below. Honor a named target directory; otherwise use the current project directory.
- **Exact text, a known filename, or a user-specified file:** Read the file or use `rg` directly. Add graph exploration only when relationships matter to the task.

Resolve the intended project root from the task context. If running in a subdirectory, check the known repository root for `.graphify/graph.json`; do not recursively scan unrelated projects. Run the commands below from that root, or pass `--graph "<root>"` to read commands. `run` and `update` take the root as a positional argument.

## Investigate with an existing graph

1. Check for `.graphify/graph.json` once when starting the investigation. If absent, continue with native file tools; do not automatically build a graph for an ordinary search.
2. Read `.graphify/graph_report.md` if available and not already read in the current context. Focus on the parts relevant to the task; do not dump a general graph summary into an unrelated answer.
3. Select the command that answers the structural question:

| Task | Command |
| --- | --- |
| Locate a feature or implementation across files | `nodesify-graphify query "authentication"` |
| Orient on a large codebase | `nodesify-graphify map --budget 2000` |
| Inspect a symbol and its connections | `nodesify-graphify explain "UserService"` |
| Find how two symbols connect | `nodesify-graphify path "AuthService" "Database"` |
| Trace connections in caller/importer direction | `nodesify-graphify path "AuthService" "Database" --directed` |
| Investigate potential impact of changing a symbol or file | `nodesify-graphify affected "UserService" --depth 3` |
| Explore nearby references | `nodesify-graphify query "UserService" --depth 3` |

4. Read the identified source files before explaining logic or editing code. Verify call direction, conditions, and relevant references in the code. Use `rg` or language-aware tools when exact or exhaustive reference coverage matters.
5. Answer the user's original question or continue the implementation. Cite actual source files and distinguish confirmed behavior from inferred graph relationships. A missing node or path does not establish that an implementation or relationship is absent.

Use the default query depth and output budget first. Narrow search terms before expanding output. Useful query options:

- `--depth <n>` and `--budget <n>` control traversal and output size.
- `--dfs` selects depth-first traversal.
- `--directed` follows stored edge direction; an undirected path is not necessarily a runtime call chain.
- `--detail high` retains EXTRACTED/DECLARED facts and excludes inferred/semantic edges; source verification still matters.
- `--cursor <n>` continues truncated query output.

Use `affected` for potential downstream change impact, rather than assuming an outward directed query finds dependents. Graph impact results may omit dynamic calls, configuration, or unsupported files.

## Freshness and failures

- Do not rebuild at session start, before each query, or after every edit. Do not start a background watcher unless requested.
- When freshness matters, use `nodesify-graphify status --graph .` and compare relevant results with current source files or known edits. Age alone is not proof that source code changed.
- If changed source makes the graph unreliable for the task, run one `nodesify-graphify update .` before relying on further graph results. After a batch of edits, refresh when continued graph investigation needs those changes or the user requested an updated graph.
- For empty results, try a focused alternative term and use `nodesify-graphify stats --graph .` if graph health is uncertain. Then inspect source directly instead of repeatedly broadening queries.
- If the CLI is unavailable, the graph is empty, or an update fails, continue the original investigation with file reads and `rg`, and mention the limitation when it affects the answer. Do not silently install packages, delete the graph/database, or repeatedly retry a failing rebuild.

## Explicit build/update workflow

When the user invokes `/graphify` or asks to build or refresh a graph:

1. If `.graphify/graph.json` is absent, run `nodesify-graphify run .`. If it exists, run `nodesify-graphify update .` to incorporate source changes. Wait for completion and inspect the result.
2. Check `nodesify-graphify stats --graph .`. An incremental update reporting zero added nodes can be successful; use the total graph statistics to assess whether the graph contains data. If the total is zero, report that no nodes were indexed and inspect supported inputs before claiming the graph is ready.
3. Read `.graphify/graph_report.md` when available. Summarize the actual node, edge, and community counts and the relevant architecture findings. Do not invent surprising connections or present stale output as a successful rebuild.
4. If the request also included an investigation question, answer it using the graph and source files. Otherwise state that the graph is ready and give a relevant example query.

If a build/update fails, report the error and keep the existing artifacts intact. Diagnose within the requested scope; avoid treating a destructive rebuild as an automatic repair.

## Codex integration

The global SessionStart hook supplies a reminder for projects with an existing graph. It does not intercept searches, invoke this skill automatically on every command, or build graphs. This skill can be selected for ordinary structural investigations without `/graphify`.

Read this SKILL.md with the available file-reading tool; no tool named `Skill` is required. Use the installed CLI. If Graphify MCP tools are already connected, equivalent query/map/explain/path/affected tools may be used; do not start or configure an MCP server just to perform a search.
