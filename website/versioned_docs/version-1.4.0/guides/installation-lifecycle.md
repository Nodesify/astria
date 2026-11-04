---
title: Install, upgrade and remove Astria
description: Scoped setup, installation diagnostics, safe upgrades, backup and complete removal.
---

# Install, upgrade and remove Astria

## Install and set up

Use Node.js 22 or newer. Install the CLI with optional dependencies enabled:

```bash
npm install -g @nodesify/astria
cd your-project
astria run . --backend none
astria install --platform codex --scope project
astria doctor
```

`--backend none` builds without paid enrichment. Embeddings are a separate opt-in model download. `doctor` checks native loading and embedding capability without downloading a model or calling an API. It also reports graph compatibility, managed file integrity, MCP registrations, permissions, git hooks and the executable found on your current PATH. `doctor --json` emits a report and exits with status 1 when a check fails. An editor can have a different PATH or sandbox; check its MCP logs after restarting it.

Prebuilt targets are Windows x64/ARM64, macOS Intel/Apple Silicon, Linux glibc x64/ARM64 and Linux musl x64. Intel macOS, Windows ARM64 and Linux musl builds omit local embeddings. Other targets, including Linux musl ARM64, are unsupported. Missing optional packages and installed binaries that cannot load have separate diagnostics.

## Choose the installation scope

`project` is the default. It writes skills, rules, extensions and MCP configuration inside the project. Codex uses `.codex/config.toml`; Pi uses `.pi/extensions/astria.mjs`. Codex loads project configuration only for trusted projects, as described in its [configuration documentation](https://developers.openai.com/codex/config-basic/).

```bash
astria install --platform claude --scope project
astria install --all --scope project
astria install --platform codex --scope user
astria doctor --scope user
```

User scope installs a platform's global skill and, where applicable, its user-wide registration or extension. It does not change the current project's instructions or hooks. Platforms with only project integrations reject user scope. Select user platforms individually; `--all` installation is project-only.

Both scopes keep a `.astria-install.json` ownership record outside graph data. Keep it on the machine that performed the installation; add it to your repository ignore rules. Shared project instructions and MCP registrations remain until their last recorded consumer is removed. Installing in another repository does not require user scope.

Standalone managed files carry recorded content fingerprints. Astria refuses to overwrite or delete customized files. Back up and move a customized file before retrying. Unrelated settings and custom MCP registrations remain in merged configuration files. An interrupted installation remains recorded so it can be repaired by rerunning install or removed explicitly. Configuration writes are atomic, and installation operations are serialized. On Windows, close the editor if its file lock prevents replacement; Astria retains the original rather than writing over it.

## Upgrade

1. Stop Astria watchers and close/restart MCP sessions that have loaded the native binary. Stop only the relevant process.
2. Back up the data and configuration described below.
3. Upgrade with the package manager used for installation:

   ```bash
   npm install -g @nodesify/astria@latest
   # Homebrew installation instead: brew upgrade nodesify/tap/astria
   ```

4. In each project, rerun `astria install --all --scope project` or reinstall the selected platforms. Repeat user-scope installation only for your chosen user integrations. Run `astria doctor` and `astria doctor --scope user` where applicable.
5. Run `astria update .` when doctor reports changed extraction rules. Reimport external indexes if reported stale. Paid policy replacement requires the explicit refresh flags and budget documented in the CLI reference.
6. Restart your assistant/MCP server and check its tool list. A running process continues using its previously loaded native binary until restarted.

The Homebrew tap is updated separately from npm. Its maintainers must update the formula version/checksum and verify the installed CLI after each release. Use an explicitly pinned npm version if the tap has not caught up.

## Backup and restore

Stop writers before copying or restoring SQLite data. Copy the entire project's `.astria/` directory, including SQLite sidecars if present: it contains curated memory, transcripts, history, graph indexes and the saved indexing profile. Copy `~/.astria/` separately if using the cross-repository store. Keep each `.astria-install.json` and back up the editor configuration files changed by setup. Store the backup outside the directory that will be purged.

Restore into the same scope with writers stopped: move the current data directory aside, copy the backed-up directory into its original location, restart Astria and run `doctor` followed by `update .` if required. Preserve curated memory and transcripts even if rebuilding a derived graph. Copy configuration backups individually rather than replacing an entire editor configuration directory. Installation records contain absolute file locations; reinstall integrations after moving a project to a different path or machine.

An embedding cache may live outside these directories, especially when `ASTRIA_EMBED_CACHE_DIR` is set. Back it up separately if avoiding a future download matters.

## Uninstall and complete removal

Plain uninstall removes integrations in the selected scope and keeps data:

```bash
astria uninstall --platform codex --scope project
astria uninstall --all --scope project
astria uninstall --platform codex --scope user
```

`--all` removes recorded platforms in the selected scope. For an installation without a record, name its platform explicitly. Current installers do not sweep obsolete locations automatically; inspect and remove obsolete files deliberately.

Data removal uses separate explicit flags:

```bash
astria uninstall --all --scope project --purge-project
astria uninstall --all --scope user --purge-global
```

`--purge-project` removes this checkout's git hooks, merge-driver wiring and `.astria/` graph data. `--purge-global` deletes the user-wide cross-repository store and affects every repository represented there. Stop watchers and MCP writers, and back up curated data first. A failed cleanup exits with status 1, reports the failure and retains graph data when earlier integration cleanup failed. Retry after resolving the reported problem.

The old combined `--purge` option is removed. Choose project/global data explicitly. Project uninstall never removes user integrations; remove those with `--scope user`. Repeat project cleanup in other checkouts as needed.

Finally remove the CLI with the package manager that installed it:

```bash
npm uninstall -g @nodesify/astria
# Local dependency instead: npm uninstall @nodesify/astria
# Homebrew instead: brew uninstall astria
```

Remove an unwanted external embedding cache separately at its known location. No uninstall command kills processes or searches other repositories for data to delete. A crashed installer can leave an installation lock in the system temporary directory; the next command reports its exact path. Inspect its recorded PID and remove that one lock only after verifying its process has stopped.
