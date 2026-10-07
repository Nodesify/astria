# Release procedure

1. Bump the CLI version, its seven exact optional dependency versions, platform package versions and lockfile together. Every npm version is immutable: use a new version for changed artifacts. Update CHANGELOG.md and server.json.
2. Verify npm trusted publishing for the CLI and all seven native packages. Each binding must use this repository, `.github/workflows/release.yml` and environment `prod`. Linux x64 musl and Windows ARM64 must have bindings before the first release that includes them. Authentication failures now block publication instead of silently omitting a target.
3. Run the release workflow manually on the intended revision. This builds all seven artifacts, installs packed packages on matching runners (musl inside Alpine), verifies native loading and graph operations, then rehearses packing. The manual run publishes nothing. Inspect every target before tagging.
4. Create the version tag only after the rehearsal passes. Publishing waits for the same packed-install gates; platform packages publish before the CLI. Windows x64 artifacts include the DirectML runtime collected from Cargo's ONNX link-search directories.
5. Verify the npm CLI and its matching platform package on a clean machine. Restart existing MCP processes before checking the installed version and run `astria doctor`.
6. Update the separate Homebrew tap with the version, tarball URL and checksum printed by the release job, verify installation, then publish the tap change. npm publication does not update that repository automatically.

If a publication fails midway, preserve the artifacts and rerun the failed job. Already published versions are skipped, but mismatched or missing native artifacts must never be replaced under an existing version. Issue a new version for artifact changes.

Runner labels use GitHub's [supported hosted runners](https://docs.github.com/en/actions/reference/runners/github-hosted-runners). Project configuration follows the assistant's native discovery rules: [Codex project configuration](https://developers.openai.com/codex/config-basic/) and [Pi extensions](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md).
