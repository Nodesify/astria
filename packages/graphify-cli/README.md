# @nodesify/graphify

Turn any folder into a queryable knowledge graph.

## Install

```bash
npm install -g @nodesify/graphify
```

Requires Node.js >= 20. No Rust toolchain needed — ships prebuilt native binaries for macOS, Linux, and Windows.

## Usage

```bash
nodesify-graphify run <path>                            # Full pipeline: detect → extract → build → cluster → analyze → report
nodesify-graphify update <path>                         # Incremental rebuild (only changed files)
nodesify-graphify watch <path> [--debounce 3000]        # Watch for file changes, auto-rebuild
nodesify-graphify explain <node> [--graph .]            # Explain a node and its connections
nodesify-graphify query <question> [--dfs] [--depth 2] [--budget 2000] [--graph .]  # BFS/DFS traversal
nodesify-graphify path <A> <B> [--graph .]              # Shortest path between two concepts
nodesify-graphify stats [--graph .]                     # Node/edge/community counts
nodesify-graphify export [--graph .] [--out graph.json] [--format json|html|graphml] [--mode standard|large] # Export graph; HTML defaults to standard
nodesify-graphify merge <pathA> <pathB> <outPath>       # Merge two graphs
nodesify-graphify diff <pathA> <pathB>                  # Compare two graphs
nodesify-graphify history [--limit 20] [--graph .]      # Show recent query history
nodesify-graphify install [--platform claude]           # Install skill files for AI coding assistants
nodesify-graphify hook install                          # Install git post-commit/post-checkout hooks
```

Running `nodesify-graphify run .` creates `.graphify/` with:

- `db.sqlite` — the graph database
- `graph.json` — full graph export
- `graph_report.md` — report with hub nodes, communities, surprising connections

### HTML visualization modes

HTML export defaults to `--mode standard`, matching the original Graphify viewer's 5,000-node safety limit. Graphs above that limit require an explicit large-graph export:

```bash
nodesify-graphify export --graph . --format html --mode large --out graph-view.html
```

Large mode uses precomputed positions and disabled physics, starts with key nodes visible, and provides debounced search plus a “Show all nodes” toggle. The `--mode` option only affects HTML export; JSON and GraphML remain unchanged.

## Supported languages

Python, JavaScript, TypeScript, Rust, Go, Java, C, C++, Ruby, Swift, Scala, PHP, C#, Lua, Haskell, Elixir, Bash, Dart, Zig, CSS.

## AI platform integration

```bash
nodesify-graphify install --platform claude   # or: codex, gemini, cursor, copilot, aider, opencode, kiro, trae, zcode
```

## .graphifyignore

Place a `.graphifyignore` file in your project root (gitignore syntax) to exclude files from the graph.

## License

MIT
