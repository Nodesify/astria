# 1.0.0 astria release — external checklist

Everything the rename itself could not do from inside the repo. Work through
top to bottom; each step is ordered so a failure can't leave a half-published
state worse than the previous one.

## 1. GitHub
- [ ] Rename repo `Nodesify/nodesify-graphify` → `Nodesify/astria`
      (Settings → General → Repository name). GitHub redirects old URLs and
      git remotes automatically; update local `origin` URLs anyway:
      `git remote set-url origin git@github.com:Nodesify/astria.git`
- [ ] Update repo description/topics (old topics mention graphify).
- [ ] GitHub Pages: the docs site URL becomes `nodesify.github.io/astria/`
      automatically once the repo renames, but **old links do not redirect**
      for Pages. Update any external links (README badges already point at
      the new path in this repo).

## 2. npm — before first publish
- [ ] On npmjs.com, open the `@nodesify` org and pre-create the package
      entries `@nodesify/astria` + the five platform packages
      (`-win32-x64-msvc`, `-darwin-x64`, `-darwin-arm64`,
      `-linux-x64-gnu`, `-linux-arm64-gnu`) with **GitHub trusted publishing**
      linked to `Nodesify/astria` and the release workflow. Brand-new package
      names + OIDC provenance fail the first publish if the package doesn't
      exist and isn't linked; if a publish is rejected, fall back to one
      manual publish from a maintainer machine (`npm publish --access public`
      in `packages/astria-cli/npm/<platform>/`, then the main package) and
      link trusted publishing afterwards.

## 3. Release
- [ ] Merge `develop` → `main` per the usual flow.
- [ ] Tag and push: `git tag v1.0.0 && git push origin v1.0.0` (from `main`).
      `release.yml` verifies the tag equals
      `packages/astria-cli/package.json` (1.0.0), builds five platforms, and
      publishes platform packages then the main package.

## 4. npm — after publish
- [ ] Deprecate the old packages so every existing install sees the pointer:
      ```
      npm deprecate @nodesify/graphify "Renamed to @nodesify/astria — npm i -g @nodesify/astria, then run `astria migrate` and `astria install`"
      npm deprecate @nodesify/graphify-win32-x64-msvc "Renamed to @nodesify/astria-win32-x64-msvc"
      npm deprecate @nodesify/graphify-darwin-x64 "Renamed to @nodesify/astria-darwin-x64"
      npm deprecate @nodesify/graphify-darwin-arm64 "Renamed to @nodesify/astria-darwin-arm64"
      npm deprecate @nodesify/graphify-linux-x64-gnu "Renamed to @nodesify/astria-linux-x64-gnu"
      npm deprecate @nodesify/graphify-linux-arm64-gnu "Renamed to @nodesify/astria-linux-arm64-gnu"
      ```
- [ ] Consider one final `@nodesify/graphify@0.9.1` whose postinstall prints
      the migration banner, if you want in-CLI reach even without deprecation
      notices.

## 5. Docs site
- [ ] If cutting a fresh docs version, follow `website/README.md`
      (`npm run docusaurus docs:version`), then point `lastVersion` in
      `website/docusaurus.config.js` at it. `versioned_docs/version-0.8.0`
      stays frozen as history (it documents the graphify era — that is
      correct).

## 6. Personal machine (partly done)
- [x] `~/.nodesify-graphify/global.db` → moved to `~/.astria/global.db`
- [x] `~/.astria-embed-cache` seeded from the old embed cache
- [x] `~/.claude/CLAUDE.md` — stale duplicated graphify registrations replaced
      with the astria registration
- [x] project skills/hooks/MCP configs refreshed via the new installer
- [ ] This repo's `.graphify/` directory: **delete it after closing editors** —
      the graphify MCP server session holds `db.sqlite` locked, which is why
      `astria migrate` couldn't rename it. Everything already lives in
      `.astria/`.

## Deliberately untouched
- `website/versioned_docs/version-0.8.0/`, `worked/` content, `blog/`,
  `TODO.md`, `PLAN.md`, `REARCHITECTURE.md`, `docs/superpowers/` — historical
  records of the graphify era.
- `scripts/bench/` upstream references (`safishamsi/graphify`) — that project
  is the benchmark comparison target, not ours.
- Legacy-format matchers in the installer — they exist to upgrade pre-1.0
  installs; do not "clean them up" for a few release cycles.
