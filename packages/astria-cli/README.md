# @nodesify/astria

Turn any folder into a queryable knowledge graph.

## Measured results (October 2026, release candidate)

Head-to-head against the original Python [Graphify](https://github.com/safishamsi/graphify) on identical corpora, pinned commits, one shared `o200k_base` tokenizer — structural pipeline only, no LLM:

| Measurement | astria | Graphify |
|---|---|---|
| Token reduction (17 MB corpus, 4.59M tokens) | **2,875×** | 2,575× |
| Avg tokens per answer (same questions, same budget) | **1,597** | 1,783 |
| Retrieval suites won or tied (file recall@5, 10 held-out/frozen suites) | **10 / 10** | — |
| Exact-declaration recall@5 | **40–100%** | ~0% |
| Over-budget answers | **0** | 6 of 10 suites |
| LoCoMo conversational-memory recall@5 (full 1,977-question set) | **83.6%** | — |

Self-corpus retrieval gate: hit@5 0.800, recall@5 0.743, zero failures, zero budget violations. Version-over-version performance (v1.2.0 → candidate): no regressions; queries 9–22% faster, explain 37% faster. Full methodology, tables, and reproduction commands: [Benchmarks and evidence](https://nodesify.github.io/astria/docs/explanation/benchmarks) and [RESULTS-2026-10-16.md](https://github.com/Nodesify/astria/blob/develop/RESULTS-2026-10-16.md).

## Install

```bash
npm install -g @nodesify/astria
```

Requires Node.js >= 22. No Rust toolchain needed — ships prebuilt native binaries for macOS, Linux, and Windows.

## Usage

```bash
astria run <path>                            # Full pipeline: detect → extract → build → cluster → analyze → report
astria update <path>                         # Reuse cached extraction; reconcile current corpus
astria watch <path> [--debounce 3000]        # Watch for file changes, auto-rebuild
astria explain <node> [--graph .]            # Explain a node and its connections
astria query <question> [--seed <symbol>]... [--dfs] [--depth 2] [--budget 2000] [--graph .]  # BFS/DFS traversal from explicit symbols
astria path <A> <B> [--graph .]              # Shortest path between two concepts
astria stats [--graph .]                     # Node/edge/community counts + extraction completeness
astria save-result <question> [--answer <text>|--answer-file <path>] [--outcome useful|dead_end|corrected] [--correction <text>] [--nodes <ids>]  # Curate a memory
astria reflect [--graph .]                   # Aggregate memories into actionable LESSONS.md
astria export [--graph .] [--out graph.json] [--format json|html|graphml] [--mode standard|large] # Export graph; HTML defaults to standard
astria merge <pathA> <pathB> <outPath>       # Merge two graphs
astria diff <pathA> <pathB>                  # Compare two graphs
astria history [--limit 20] [--graph .]      # Show recent query history
astria install [--platform claude]           # Install skill files for AI coding assistants
astria hook install                          # Install git post-commit/post-checkout/post-merge hooks
```

Running `astria run .` creates `.astria/` with:

- `db.sqlite` — the graph database
- `graph.json` — full graph export
- `graph_report.md` — report with hub nodes, communities, surprising connections
- `memory/` + `reflections/LESSONS.md` — the curated memory loop (see below)

### Explicit query seeds

`astria query "..." --seed <symbol>` (repeatable; also a `seeds` argument on the MCP `query_graph` tool) anchors the traversal at known symbols instead of keyword ranking — node id, label, bare name, or a unique source path all resolve. Unresolved seeds are reported (`unresolvedSeeds` in JSON, a header line in text), and when every supplied seed fails to resolve the query returns an explicit miss rather than silently falling back to keyword search.

### Retrieval and upgrades

Scoped code definitions survive semantic deduplication. JS/TS assigned functions retain binding names and bodies; Python implementations take precedence over overload declarations. Queries favor complete identifiers while honoring explicit requests for tests or documentation. Static receiver matching does not infer runtime types or aliases.

TypeScript member calls resolve through declared types — constructor parameter properties, typed fields, locals, and `new` initializers — including inherited members; ambiguous receivers stay unresolved, and these bindings are name/type-derived (`RESOLVED`), never compiler-proven. Interfaces are extracted with their members and dispatch to every implementing class via `dispatch` edges, marked INFERRED: possible implementations, not proven runtime calls. Member calls whose receiver type is known but whose member lives in another repository retain `receiver::type::<T>::<m>` evidence that the cross-repo global resolver links on a unique name+member match.

`query --budget N` caps the full query text using `o200k_base`, including metadata. CLI and MCP share that text budget; MCP transport JSON is excluded. Budgets must fit metadata and the next complete record or the query returns an error. Use the returned `--cursor N` for the next page of node and edge records.

Extraction completeness is tracked per file — complete, partial (with the first parser-error lines), unsupported, skipped, or suspicious-empty — and surfaced by `astria stats`, `astria diagnose`, and the MCP `graph_stats` tool, so a missing relationship is distinguishable from an incompletely-understood file. Community detection weights edges by their stored evidence (EXTRACTED > RESOLVED > INFERRED > SEMANTIC) so weak links cannot outvote strong structure, and query-usage `learned` edges never shape module boundaries.

After upgrading, run `astria update .` to refresh extraction and restore definitions removed by older deduplication. Discard old pagination cursors after an upgrade or graph rebuild.

### Project configuration (`.astria.toml`)

```toml
[languages]
".inc" = "PHP"      # remap an ambiguous suffix to a registered language
".tpl" = "TypeScript"
```

Remapped suffixes flow consistently through discovery, parser selection, watch, and cache identity; invalid entries warn and are skipped, and changing a mapping re-extracts exactly the affected files.

### Memory loop: save-result and reflect

`astria save-result` records a Q/A verdict (useful / dead_end / corrected, optionally a correction and cited node ids) into `.astria/memory/`. `astria reflect` aggregates those memories into `.astria/reflections/LESSONS.md` as actionable output: preferred sources (corroborated by multiple useful memories, most recent first), corrections to apply, known dead ends, contested evidence, and tentative findings awaiting verification — with citations of deleted nodes discarded automatically.

Verdicts also gate the learned-edge loop: a node pair promoted from repeated queries must be backed by at least one question that was not judged a dead end or correction, so repeated unsuccessful exploration never becomes an authoritative edge. Lessons stay experiential — they never merge into the structural graph facts.

### HTML visualization modes

HTML export defaults to `--mode standard`, matching the original Graphify viewer's 5,000-node safety limit. Graphs above that limit require an explicit large-graph export:

```bash
astria export --graph . --format html --mode large --out graph-view.html
```

Large mode uses precomputed positions and disabled physics, starts with key nodes visible, and provides debounced search plus a “Show all nodes” toggle. The `--mode` option only affects HTML export; JSON and GraphML remain unchanged.

## Supported languages

Python, JavaScript, TypeScript, Rust, Go, Java, C, C++, Ruby, Swift, Kotlin, Scala, PHP, C#, Lua, Haskell, Elixir, Bash, Dart, Zig, CSS, Terraform/HCL, PowerShell, Verilog/SystemVerilog, and Metal. See the [generated language table](https://nodesify.github.io/astria/docs/reference/language-support) for extensions and extraction details.

## AI platform integration

```bash
astria install --platform claude   # or: codex, gemini, cursor, copilot, aider, opencode, kiro, trae, zcode
```

## .astriaignore

Place a `.astriaignore` file in your project root (gitignore syntax) to exclude files from the graph.

## License

MIT

Installation defaults to project scope: `astria install --platform codex --scope project`. Use `--scope user` explicitly for user-wide integrations. Run `astria doctor` after setup or upgrades. See the [installation lifecycle guide](https://nodesify.github.io/astria/docs/guides/installation-lifecycle) for supported platforms, upgrade, backup/restore and complete removal.
