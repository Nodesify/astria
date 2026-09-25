---
name: astria
description: Turn any directory into a queryable knowledge graph. Trigger: /astria
---

# astria skill (Aider)

When the user types `/astria`, run the astria knowledge graph pipeline.

## Step 1 - Build or update the graph

Run in terminal:
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


## Usage with Aider

Aider focuses on code editing. Use astria to understand the codebase before making changes:

```bash
astria query "authentication flow"    # understand a feature
astria explain "UserService"           # see what a class does
astria path "Config" "Database"        # trace dependencies
astria affected "UserService"          # blast radius of a change
```

After editing, run `astria update .` to keep the graph current.
