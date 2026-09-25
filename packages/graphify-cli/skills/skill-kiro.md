---
name: graphify
description: Turn any directory into a queryable knowledge graph. Trigger: /graphify
---

# graphify skill (Kiro)

When the user types `/graphify`, run the nodesify-graphify knowledge graph pipeline.

## Step 1 - Build or update the graph

Run via terminal:
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

The `.kiro/steering/graphify.md` steering file enforces graph usage. You MUST:

1. Read `.graphify/graph_report.md` before searching files
2. Use `nodesify-graphify query`, `path` (add `--directed` for call direction), `explain`, or `affected` for cross-module questions
3. Only use native file tools AFTER the graph identified the exact files

## After editing code

Run `nodesify-graphify update .` to keep the graph current.
