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

If the `astria` MCP server is connected (registered by `astria install` for most coding tools — claude, cursor, gemini, zcode, vscode, codex, trae, kiro, windsurf, opencode, and the Copilot coding agent)), prefer its native tools over shell commands: `repo_map` to orient, `query_graph` for a natural-language question, `explain`/`get_neighbors` for one symbol, `shortest_path` to trace a connection, `affected` before changing a shared symbol. Use the CLI commands below only when the server is not connected.


**Bypass check (hub/bridge questions only).** A hub question's neighbour listing (`astria explain`, `astria god-nodes`) only shows code that already has an edge to the hub - it cannot show code that does the hub's job without ever calling it. When answering why a node bridges communities or has unusually many edges: pick one or two literal, distinctive things the hub does (a hard-coded constant it wraps, a specific string it returns, the exact computation in its body), then grep the corpus for that literal text in files with no edge to the hub in the traversed subgraph. Report hits separately, labelled "Bypasses" - each is a file that reimplements the hub instead of using it. Also check whether the hub's community label names its heaviest consumer rather than the hub itself (a shared utility is often clustered into its biggest user's community); if so, say so plainly instead of letting the label stand.


## Usage with Aider

Aider focuses on code editing. Use astria to understand the codebase before making changes:

```bash
astria query "authentication flow"    # understand a feature
astria explain "UserService"           # see what a class does
astria path "Config" "Database"        # trace dependencies
astria affected "UserService"          # blast radius of a change
```

After editing, run `astria update .` to keep the graph current (git hooks installed via `astria hook install` do this automatically after every commit). To share the graph, `astria export --format html --out graph.html` writes a self-contained interactive bubble viewer — click to expand, search to jump, click a node to focus its relation-labeled neighbors.
