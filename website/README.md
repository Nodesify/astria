# nodesify-graphify docs site

Docusaurus v3 site deployed to GitHub Pages at
https://nodesify.github.io/nodesify-graphify/ via
`.github/workflows/docs.yml` (builds on every push touching `website/**`
to `main` or `develop`, plus manual `workflow_dispatch`).

## Local development

```bash
cd website
npm install
npm start        # dev server with hot reload
npm run build    # production build (what CI runs)
npm run serve    # serve the production build locally
```

## Cutting a new docs version

The published site always shows the **latest release** by default
(`lastVersion` in `docusaurus.config.js`). `website/docs/` is the
in-progress "Next" version and carries an unreleased banner.

When a release ships (e.g. v0.9.0):

```bash
cd website
npm run docusaurus docs:version 0.9.0
```

This snapshots `docs/` into `versioned_docs/version-0.9.0/` and adds the
version to `versions.json`. Then update `docusaurus.config.js`:

1. `lastVersion: '0.9.0'`
2. add a `'0.9.0': { banner: 'none' }` entry under `versions`
3. add a `banner: 'unreleased'`-style entry for the new Next state if needed

Commit and push — CI redeploys.

## Search

Local build-time search via `@easyops-cn/docusaurus-search-local` — indexes
all docs versions at build, no external service. To switch to Algolia
DocSearch (apply at https://docsearch.algolia.com/), remove the `themes`
block from `docusaurus.config.js` and add an `algolia` key to `themeConfig`
with the appId/apiKey/indexName Algolia gives you.

## Benchmark snapshot

`docs/benchmarks.md` renders `src/data/benchmarks-snapshot.json` (via
`src/components/BenchmarkSnapshot`). To refresh it: **Actions → Benchmark
snapshot → Run workflow** — it runs both tools (ours + the original Python
graphify pinned to `91f4d12`) on a fresh runner via
`scripts/bench/run-snapshot.mjs`, commits the updated JSON, and dispatches
this site's deploy. Locally you can run the same with
`node scripts/bench/run-snapshot.mjs` (needs `uv`, and the published CLI).

## Social preview image

`static/img/og-image.png` (1200×630) is referenced by the `og:image` /
`twitter:image` metadata in `docusaurus.config.js` and is also suitable as
the GitHub repo social preview (Settings → General → Social preview).

To regenerate it after a rebrand: open `scripts/og-image.html` in a browser
at a 1200×630 viewport and take a full-viewport screenshot over
`static/img/og-image.png`.
