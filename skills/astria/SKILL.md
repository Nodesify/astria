---
name: astria
description: Turn any directory into a queryable knowledge graph of a codebase - hub files, communities, blast radius, and natural-language questions like "how does authentication work". Use before exploring an unfamiliar repo, tracing how modules connect, or assessing what breaks if a symbol changes. Self-contained - detects whether the astria CLI is installed and guides a one-command install when it isn't; reading an existing graph needs no install at all.
license: MIT
metadata:
  author: Nodesify
  version: "1.0.0"
  homepage: https://nodesify.github.io/astria
---

# astria: query the codebase as a knowledge graph

astria turns a folder of source code into a queryable graph stored in `.astria/` - deterministic AST extraction via tree-sitter, PageRank hub ranking, community detection, and a plain-language report. Questions that cost grep-and-read sessions ("where does auth live?", "what breaks if I change this?") become one graph query that answers in ~3,000 tokens.

## Step 0 - Check the environment (do this first)

Run the CLI check via your shell:

```bash
astria --version
```

Then branch on the result:

**A. Command found** - astria is installed. Continue to the workflow below.

**B. Command not found, but `.astria/` exists** - the graph is still usable in read-only mode with no install: `.astria/graph_report.md` is a self-contained report (hub nodes, communities, surprising connections), and if `.astria/wiki/` exists it is a plain-markdown wiki you can navigate by reading files. Answer from those files directly and say the graph may be stale; offer the install below if the user wants queries.

**C. Command not found and no `.astria/`** - tell the user astria isn't installed in this environment and offer the install. Ask before installing - do not install unprompted:

> astria isn't installed here. Install it with `npm install -g @nodesify/astria` (prebuilt binaries, no Rust toolchain, needs Node >= 20)? After that I can build the graph and answer graph questions.

If the user declines, stop cleanly and leave the command for later. Do not fake graph answers by guessing from file reads - say the graph is unavailable instead.

## The graph-first workflow (when installed)

If `.astria/` exists in the project, prefer the graph over grep/glob for architecture questions. The graph tells you *which* files matter (hubs, blast radius, cross-module paths) and every answer carries `file:line` anchors - then read those files directly for exact logic.

| User asks | Command |
|-----------|---------|
| Where is X implemented? | `astria query "X"` |
| How does X connect to Y? | `astria path "X" "Y"` |
| What breaks if X changes? | `astria affected "X"` |
| What does X do? | `astria explain "X"` |
| Architecture overview? | Read `.astria/graph_report.md` (or `.astria/wiki/index.md`) |
| Best first orientation | `astria map` |

Keep native search for what the graph can't see: predicate-level bugs, exact-string audits, and cold discovery when no symbol name is known yet.

If an `astria` MCP server is connected (registered by `astria install` for claude, cursor, gemini, and zcode), prefer its native tools - `repo_map`, `query_graph`, `explain`, `get_neighbors`, `shortest_path`, `affected` - over shell commands.

## Build or refresh the graph

```bash
astria run .       # full build, creates .astria/ (db.sqlite, graph.json, graph_report.md)
astria update .    # incremental refresh after code changes
```

If the project has no `.astria/` yet, run `astria run .` first (ask the user if the repo is large). Supported languages include Python, JS, TS, Rust, Go, Java, C, C++, Ruby, Swift, Kotlin, Scala, PHP, C#, Lua, Haskell, Elixir, Bash, Dart, Zig, and CSS.

After editing code in a session with an active graph, run `astria update .` so later answers reflect the change.

## Command quick reference

- `astria query "<question>"` - BFS traversal from matching nodes (`--dfs`, `--depth`, `--budget`, `--directed`, `--detail high` for declared-facts-only)
- `astria map` - PageRank-ranked repo map with each file's top symbols; best first command on an unfamiliar repo
- `astria explain <node>` - node details with all connections and `file:line` anchors
- `astria path <A> <B>` - shortest path; add `--directed` for call direction
- `astria affected <node>` - blast radius (reverse reachability over calls/imports/uses)
- `astria stats` / `astria status` - graph health and staleness
- `astria wiki --graph .` - markdown wiki any agent can read without the CLI

## Troubleshooting

- **Empty graph (0 nodes)** - run `astria run .`; check the directory has supported file types
- **Query finds nothing** - use broader/partial terms ("auth" not "authenticateUserWithOAuth"); verify with `astria stats`
- **Answers look stale** - run `astria update .`, or `astria run .` for a full rebuild

## Links

- Docs: https://nodesify.github.io/astria
- Repo: https://github.com/Nodesify/astria
- npm: `@nodesify/astria`
