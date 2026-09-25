# @nodesify/astria

Turn any folder into a queryable knowledge graph.

## Install

```bash
npm install -g @nodesify/astria
```

Requires Node.js >= 20. No Rust toolchain needed — ships prebuilt native binaries for macOS, Linux, and Windows.

> **Migrating from `@nodesify/graphify`?** Run `astria migrate` once (renames `.graphify/` → `.astria/` and the global store), then `astria install` to refresh AI-tool skills and hooks. `GRAPHIFY_*` env vars keep working; `ASTRIA_*` takes precedence.

## Usage

```bash
astria run <path>                            # Full pipeline: detect → extract → build → cluster → analyze → report
astria update <path>                         # Incremental rebuild (only changed files)
astria watch <path> [--debounce 3000]        # Watch for file changes, auto-rebuild
astria explain <node> [--graph .]            # Explain a node and its connections
astria query <question> [--dfs] [--depth 2] [--budget 2000] [--graph .]  # BFS/DFS traversal
astria path <A> <B> [--graph .]              # Shortest path between two concepts
astria stats [--graph .]                     # Node/edge/community counts
astria export [--graph .] [--out graph.json] [--format json|html|graphml] [--mode standard|large] # Export graph; HTML defaults to standard
astria merge <pathA> <pathB> <outPath>       # Merge two graphs
astria diff <pathA> <pathB>                  # Compare two graphs
astria history [--limit 20] [--graph .]      # Show recent query history
astria install [--platform claude]           # Install skill files for AI coding assistants
astria hook install                          # Install git post-commit/post-checkout hooks
```

Running `astria run .` creates `.astria/` with:

- `db.sqlite` — the graph database
- `graph.json` — full graph export
- `graph_report.md` — report with hub nodes, communities, surprising connections

### HTML visualization modes

HTML export defaults to `--mode standard`, matching the original Graphify viewer's 5,000-node safety limit. Graphs above that limit require an explicit large-graph export:

```bash
astria export --graph . --format html --mode large --out graph-view.html
```

Large mode uses precomputed positions and disabled physics, starts with key nodes visible, and provides debounced search plus a “Show all nodes” toggle. The `--mode` option only affects HTML export; JSON and GraphML remain unchanged.

## Supported languages

Python, JavaScript, TypeScript, Rust, Go, Java, C, C++, Ruby, Swift, Scala, PHP, C#, Lua, Haskell, Elixir, Bash, Dart, Zig, CSS.

## AI platform integration

```bash
astria install --platform claude   # or: codex, gemini, cursor, copilot, aider, opencode, kiro, trae, zcode
```

## .astriaignore

Place a `.astriaignore` file in your project root (gitignore syntax) to exclude files from the graph.

## License

MIT
