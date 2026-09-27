# Contributing to astria

Thanks for helping improve astria! This document covers setup, testing, and the conventions the project follows.

## Development setup

Prerequisites:

- **Rust** 1.88+ (enforced via `rust-version` in the workspace) — `rustup` recommended
- **Node.js** 22+ (matches the badge, engines floor, and CI matrix) and npm

```bash
git clone https://github.com/Nodesify/astria.git
cd astria

# Build the Rust core and run its tests
cargo build --release
cargo test

# Make the built native module available to the source CLI (Linux)
mkdir -p packages/astria-cli/dist
cp target/release/libastria_napi.so packages/astria-cli/dist/astria.node
# macOS: copy target/release/libastria_napi.dylib to the same destination
# Windows PowerShell: New-Item -ItemType Directory -Force packages/astria-cli/dist
# Windows PowerShell: Copy-Item target/release/astria_napi.dll packages/astria-cli/dist/astria.node

# Build the Node.js CLI and run its tests
cd packages/astria-cli
npm ci
npm run build
npm test   # includes an end-to-end test against the compiled binary (needs dist/)
```

The source CLI must load the native artifact built from the same checkout. A `packages/astria-cli/astria.node` file takes precedence over `dist/astria.node`; keep that path absent or update it to the same build before measuring source behavior. See the [quality benchmark methodology](scripts/bench/quality/README.md) for provenance and comparison rules.

### Documentation site

The docs live in `website/` (Docusaurus, versioned):

```bash
cd website
npm ci
npm start          # dev server
npm run build      # strict build — onBrokenLinks is 'throw', so broken links fail CI
```

When you change behavior, update the docs in `website/docs/` in the same PR. Note that released versions are frozen under `website/versioned_docs/` — fix those only for factual errors that would mislead users of that release.

## Project layout

- `crates/` — the Rust workspace (16 crates with separate responsibilities; see the crate list in the [README](README.md) or the [architecture docs](https://nodesify.github.io/astria/docs/explanation/architecture))
- `crates/astria-extract/src/langs/` — one config module per supported language; export a new module in `langs/mod.rs`, register names/extensions/parser once in `crates/astria-core/src/languages.rs`, then regenerate documentation with `node scripts/generate-language-support.mjs`
- `packages/astria-cli/` — the Commander-based CLI
- `website/` — the documentation site
- `worked/` — worked examples and benchmarks, including honest head-to-head data

Pipeline stages separate extraction, persistence, and derived outputs. Keep extraction deterministic: anything an LLM produces must be opt-in and clearly provenance-labeled (`INFERRED`, never `EXTRACTED`).

## Conventions

- **Commits:** [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix(cli):`, `docs(site):`, …), as used in the existing history.
- **Tests:** every bug fix gets a regression test; new features ship with unit tests (in-memory SQLite + `tempfile` fixtures) and, where relevant, integration coverage in `crates/astria-napi/tests/`.
- **Security:** never shell out with string interpolation, never read or store secret *values* (env var *names* are fine), validate URLs against the SSRF rules in `crates/astria-ingest`, and render exported labels as text. See [SECURITY.md](SECURITY.md) for the threat model.
- **Docs:** user-facing changes update `website/docs/`; the README links out to the docs rather than duplicating them.

## Licensing and the CLA

astria is MIT-licensed. To keep the project free to maintain, relicense, or
extend its own code later without tracking down every past contributor,
contributions are accepted under the [Contributor License Agreement](CLA.md) —
a lightweight agreement where you keep full ownership and the project gets the
standard license to use your work. **Submitting a pull request constitutes
your agreement to it.** No signature form is required.

## Submitting changes

1. Fork / branch from `develop`.
2. Make the change with tests and docs (accepted under the [CLA](CLA.md) by opening the PR).
3. `cargo test` and `cd packages/astria-cli && npm test` must pass.
4. `cd website && npm run build` must pass if you touched the docs.
5. Open a PR describing *what* and *why*; link any related issues.

## Reporting bugs and security issues

- Bugs and feature requests: [open an issue](https://github.com/Nodesify/astria/issues/new/choose) — the templates ask for the output we need.
- Security vulnerabilities: please **do not** open a public issue — see [SECURITY.md](SECURITY.md).

## Release process (maintainers)

Releases are tagged (`vX.Y.Z`) and published by the `Release` workflow via npm trusted publishing; platform binaries are built as `optionalDependencies`.
