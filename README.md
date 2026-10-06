<div align="center">

# astria

**Knowledge graph builder for codebases**

[![CI](https://github.com/Nodesify/astria/actions/workflows/ci.yml/badge.svg)](https://github.com/Nodesify/astria/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/@nodesify/astria)](https://www.npmjs.com/package/@nodesify/astria)
[![npm downloads](https://img.shields.io/npm/dm/@nodesify/astria)](https://www.npmjs.com/package/@nodesify/astria)
[![docs](https://img.shields.io/badge/docs-latest-blue)](https://nodesify.github.io/astria/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Node](https://img.shields.io/badge/node-22-339933?logo=nodedotjs&logoColor=white)](https://nodejs.org/)
[![Ask DeepWiki](.github/assets/deepwiki-badge.svg)](https://deepwiki.com/Nodesify/astria)

[Docs](https://nodesify.github.io/astria/) | [Getting started](https://nodesify.github.io/astria/docs/getting-started) | [CLI Reference](https://nodesify.github.io/astria/docs/reference/cli) | [Architecture](ARCHITECTURE.md) | [Worked examples](worked/) | [Changelog](CHANGELOG.md) | [Release notes](https://nodesify.github.io/astria/blog) | [Security](SECURITY.md)

Built by [Nodesify](https://nodesify.com)

</div>

Find an implementation, inspect its relationships, and assess a change before editing. Astria builds a local SQLite knowledge graph with deterministic Rust/tree-sitter extraction. CLI and MCP queries use the same retrieval engine; source locations and evidence classes make results inspectable. Structural analysis needs no API key. Local embeddings and remote semantic enrichment are optional.

## Install

```sh
npm install -g @nodesify/astria
```

Node.js >= 22 is required. Prebuilt native binaries ship with the package; installation needs no Rust toolchain. macOS/Linux can also use `brew install nodesify/tap/astria` ([tap](https://github.com/Nodesify/homebrew-tap)).

Intel Mac (`darwin-x64`), musl Linux and `windows-arm64` builds omit local embeddings because ONNX Runtime has no prebuilt runtime there. Structural analysis works; `run --embed` reports that embeddings are unsupported. Check `astria stats --json` for `embeddingsSupported`.

## Use the graph

```sh
astria run .                                # build .astria/
astria map                                  # orient by source modules
astria query "where is request authentication implemented"
astria explain <node-id>                     # inspect a returned symbol
astria path <caller-id> <callee-id>           # follow relationships
astria affected <node-id>                    # assess a proposed change
astria update .                              # refresh after editing
```

Use node IDs returned by the graph. Confirm important results against the linked source: name-derived call/import bindings are `RESOLVED`, and ambiguous targets remain `INFERRED`. `--detail high` restricts traversal to `EXTRACTED`/`DECLARED` facts. `affected` reports graph reachability; it cannot prove runtime behavior.

Exclude files with `.astriaignore` (gitignore syntax). Graphs and reports live under `.astria/`; read `graph_report.md` for orientation. Unchanged source reuses extraction caches, while updates reconcile references across the current corpus. After upgrading, run `astria update .` to refresh cached extraction for qualified call targets.

## Guides and evidence

- [Install and first graph](https://nodesify.github.io/astria/docs/getting-started)
- [Agent integration, MCP, skills and hooks](https://nodesify.github.io/astria/docs/guides/mcp-and-agents)
- [Wiki and exports](https://nodesify.github.io/astria/docs/guides/wiki-and-exports), [semantic enrichment](https://nodesify.github.io/astria/docs/guides/semantic-enrichment), [memory and learning](https://nodesify.github.io/astria/docs/guides/memory-and-learning)
- [CLI reference](https://nodesify.github.io/astria/docs/reference/cli), [graph model](https://nodesify.github.io/astria/docs/reference/graph-model), [troubleshooting](https://nodesify.github.io/astria/docs/reference/troubleshooting)
- [Worked examples](worked/) and [retrieval validation](website/docs/explanation/retrieval-validation.md), including known limitations

The [evaluation harness](scripts/bench/quality/README.md) separates file and declaration retrieval, delivered tokens, search/read cost and graph build time. Historical comparisons used tiny, exercised external sets and a single-pass search floor. New Requests and Commander questions are source-grounded, agent-authored, reserved and unexercised until their first explicit evaluation. No result establishes broad superiority or savings over a complete agent task.

## Architecture and languages

Rust workspace with 20 crates plus a Node.js CLI. Uses 42 registered language configurations; exact parser coverage and optional `lang-*` build features are described in [language support](https://nodesify.github.io/astria/docs/reference/language-support). See [ARCHITECTURE.md](ARCHITECTURE.md) for pipeline, caching and evidence contracts.

Astria is an independent implementation inspired by [Graphify](https://github.com/safishamsi/graphify), and is not affiliated with or endorsed by that project.

## Build from source

```bash
# Build Rust core
cargo build --release

# Copy the native library for the source CLI (Linux)
mkdir -p packages/astria-cli/dist
cp target/release/libastria_napi.so packages/astria-cli/dist/astria.node

# Build Node.js CLI
cd packages/astria-cli && npm ci && npm run build
```

See [Contributing](CONTRIBUTING.md#development-setup) for macOS and Windows native artifact paths.

Requires Rust 1.88+ (declared as `rust-version` in the workspace) and Node.js >= 22.

## License

MIT — see [LICENSE](LICENSE).

Contributions are welcome and accepted under the [Contributor License Agreement](CLA.md) — see [CONTRIBUTING.md](CONTRIBUTING.md) to get started.
