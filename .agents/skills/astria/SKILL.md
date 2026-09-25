---
name: astria
description: Turn any directory into a queryable knowledge graph. Trigger: /astria
---

# astria skill (Codex)

When the user types `/astria`, run the astria knowledge graph pipeline.

## Step 1 - Build or update the graph

Run via Bash:
```bash
node -e "const fs=require('fs');if(!fs.existsSync('.astria/graph.json')){console.log('missing')}else{const age=Math.round((Date.now()-fs.statSync('.astria/graph.json').mtimeMs)/60000);console.log(age>30?'stale':'fresh')}"
```

- `missing` → run `astria run .`
- `stale` → run `astria update .`
- `fresh` → skip to Step 2

## Step 2 - Read the report

Read `.astria/graph_report.md` and summarize: hub nodes, communities, surprising connections.
## Prefer MCP tools when connected

If the `astria` MCP server is connected (registered by `astria install` for claude, cursor, gemini, and zcode), prefer its native tools over shell commands: `repo_map` to orient, `query_graph` for a natural-language question, `explain`/`get_neighbors` for one symbol, `shortest_path` to trace a connection, `affected` before changing a shared symbol. Use the CLI commands below only when the server is not connected.


## Enforcement Rules

When `.astria/` exists, you MUST use astria commands BEFORE using Bash grep/find:

| Question | Command |
|----------|---------|
| Where is X? | `astria query "X"` |
| How does X connect to Y? | `astria path "X" "Y"` |
| What breaks if X changes? | `astria affected "X"` |
| How does X reach Y (call direction)? | `astria path "X" "Y" --directed` |
| What does X do? | `astria explain "X"` |
| Architecture overview? | Read `.astria/graph_report.md` |

Only use native file tools AFTER the graph identified the exact files.

## After editing code

Run `astria update .` to keep the graph current.
