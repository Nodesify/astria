# Contributing to astria

Thanks for helping improve astria! This document covers setup, testing, and the conventions the project follows.

## Development setup

Prerequisites:

- **Rust** 2021 edition (1.56+) — `rustup` recommended
- **Node.js** 20+ (22 recommended) and npm

```bash
git clone https://github.com/Nodesify/astria.git
cd astria

# Build the Rust core and run its tests
cargo build --release
cargo test

# Build the Node.js CLI and run its tests
cd packages/astria-cli
npm ci
npm run build
npm test   # includes an end-to-end test against the compiled binary (needs dist/)
```

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

- `crates/` — the Rust workspace (15 crates, one pipeline stage each; see the crate list in the [README](README.md) or the [architecture docs](https://nodesify.github.io/astria/docs/explanation/architecture))
- `crates/astria-extract/src/langs/` — one config module per supported language; adding a language means adding a file there and registering it in `langs/mod.rs`
- `packages/astria-cli/` — the Commander-based CLI
- `website/` — the documentation site
- `worked/` — worked examples and benchmarks, including honest head-to-head data

Each pipeline stage is a pure function in its own crate. Keep extraction deterministic: anything an LLM produces must be opt-in and clearly provenance-labeled (`INFERRED`, never `EXTRACTED`).

## Conventions

- **Commits:** [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix(cli):`, `docs(site):`, …), as used in the existing history.
- **Tests:** every bug fix gets a regression test; new features ship with unit tests (in-memory SQLite + `tempfile` fixtures) and, where relevant, integration coverage in `crates/astria-napi/tests/`.
- **Security:** never shell out with string interpolation, never read or store secret *values* (env var *names* are fine), validate URLs against the SSRF rules in `crates/astria-ingest`, and render exported labels as text. See [SECURITY.md](SECURITY.md) for the threat model.
- **Docs:** user-facing changes update `website/docs/`; the README links out to the docs rather than duplicating them.

## Submitting changes

1. Fork / branch from `develop`.
2. Make the change with tests and docs.
3. `cargo test` and `cd packages/astria-cli && npm test` must pass.
4. `cd website && npm run build` must pass if you touched the docs.
5. Open a PR describing *what* and *why*; link any related issues.

## Reporting bugs and security issues

- Bugs and feature requests: [open an issue](https://github.com/Nodesify/astria/issues/new/choose) — the templates ask for the output we need.
- Security vulnerabilities: please **do not** open a public issue — see [SECURITY.md](SECURITY.md).

## Release process (maintainers)

Releases are tagged (`vX.Y.Z`) and published by the `Release` workflow via npm trusted publishing; platform binaries are built as `optionalDependencies`.
