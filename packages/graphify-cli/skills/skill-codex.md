---
name: graphify
description: Turn any directory into a queryable knowledge graph. Trigger: /graphify
---

# graphify skill (Codex)

When the user types `/graphify`, run the nodesify-graphify knowledge graph pipeline.

## Step 1 - Build or update the graph

Run via Bash:
```bash
node -e "const fs=require('fs');if(!fs.existsSync('.graphify/graph.json')){console.log('missing')}else{const age=Math.round((Date.now()-fs.statSync('.graphify/graph.json').mtimeMs)/60000);console.log(age>30?'stale':'fresh')}"
```

- `missing` → run `nodesify-graphify run .`
- `stale` → run `nodesify-graphify update .`
- `fresh` → skip to Step 2

## Step 2 - Read the report

Read `.graphify/graph_report.md` and summarize: hub nodes, communities, surprising connections.
## Prefer MCP tools when connected

If the `graphify` MCP server is connected (registered by `nodesify-graphify install` for claude, cursor, gemini, and zcode), prefer its native tools over shell commands: `repo_map` to orient, `query_graph` for a natural-language question, `explain`/`get_neighbors` for one symbol, `shortest_path` to trace a connection, `affected` before changing a shared symbol. Use the CLI commands below only when the server is not connected.


## Enforcement Rules

When `.graphify/` exists, you MUST use graphify commands BEFORE using Bash grep/find:

| Question | Command |
|----------|---------|
| Where is X? | `nodesify-graphify query "X"` |
| How does X connect to Y? | `nodesify-graphify path "X" "Y"` |
| What breaks if X changes? | `nodesify-graphify affected "X"` |
| How does X reach Y (call direction)? | `nodesify-graphify path "X" "Y" --directed` |
| What does X do? | `nodesify-graphify explain "X"` |
| Architecture overview? | Read `.graphify/graph_report.md` |

Only use native file tools AFTER the graph identified the exact files.

## After editing code

Run `nodesify-graphify update .` to keep the graph current.
