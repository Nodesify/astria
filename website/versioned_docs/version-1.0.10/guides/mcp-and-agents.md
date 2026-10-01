---
sidebar_position: 3
title: Agent integration
description: Connect AI coding assistants to the graph — MCP server, skill-file installers for ten platforms, git hooks, and the editor-side hook-guard.
keywords: [agents, mcp, claude, codex, cursor, hooks, hook-guard, skill files]
---

# Agent integration

The graph is most useful when your AI assistant reaches for it automatically. There are four integration surfaces, all local: the MCP server, skill-file installers, git hooks, and an editor-side guard.

## MCP server

```bash
astria mcp [--graph .]
```

Runs the MCP stdio server — ten tools for querying and assessing the graph from any MCP-capable agent. The full tool list, arguments, and example calls are in the [MCP tools reference](../reference/mcp-tools). Add it to your agent's MCP config:

```json
{
  "mcpServers": {
    "astria": { "command": "astria", "args": ["mcp"] }
  }
}
```

## Skill files (`install`)

```bash
astria install [--platform claude] [--all]
astria uninstall [--platform claude] [--all]
```

Supported platforms: `claude`, `codex`, `gemini`, `cursor`, `copilot`, `aider`, `opencode`, `kiro`, `trae`, `zcode`, `vscode`, `windsurf`, `cline`, `roo`, `amp`, `pi`. `--all` installs (or removes) every platform in one run — multi-tool users don't need to know the flag per tool.

`install` writes the platform's skill files and injects an always-on `## astria` instruction block into `AGENTS.md` / `CLAUDE.md` — telling agents to query the graph before grepping and to run `update` after edits. The instruction block names both access paths: MCP tools when the `astria` server is connected, or the `astria` CLI from any agent.

Most platforms get the astria MCP server registered automatically. Project-scoped JSON configs: `zcode` (`.zcode/config.json`), `claude` (`.mcp.json` — Claude Code asks you to approve it once), `cursor` (`.cursor/mcp.json`), `gemini` (`.gemini/settings.json`), `vscode` (`.vscode/mcp.json` — native workspace MCP, what Copilot inside VS Code uses), `trae` (`.trae/mcp.json`), `windsurf` (`.windsurf/mcp.json`), `kiro` (`.kiro/settings/mcp.json` — Kiro's documented workspace-scope config), and `opencode` (`.opencode/opencode.json`; the OpenCode freshness plugin drops into the auto-discovered `.opencode/plugins/`). The Copilot coding agent has no committed MCP config file — repository-level MCP for it is JSON pasted into the repository's Settings on github.com, so `install --platform copilot` writes the skill and instructions only. `pi` installs an auto-discovered extension (`~/.pi/agent/extensions/astria.mjs`) that registers the graph as **native pi tools** (`astria_query`, `astria_map`, `astria_explain`, `astria_path`, `astria_affected`) — pi's own philosophy is registered CLI-backed tools over MCP definitions, which also costs far less context — plus graph-refresh after write/edit and an `/astria` guidance command. Users of the `pi-mcp-adapter` extension additionally get the standard `.mcp.json` tools it reads. Codex defines MCP servers in its user-global `~/.codex/config.toml`, so `install --platform codex` appends a managed `[mcp_servers.astria]` table there — never touching a hand-written one. The ten tools (`repo_map`, `query_graph`, `explain`, `get_neighbors`, `shortest_path`, `affected`, `god_nodes`, `list_communities`, `graph_stats`, `health`) then appear natively in every session for that project. All steps are idempotent and merge-safe (existing servers and unrelated config keys are preserved); `uninstall` removes them, and configs written at a since-corrected path by earlier builds are migrated away rather than duplicated.

Existing installs upgrade in place: `install` recognizes its own previously generated instruction blocks and refreshes them to the current wording; hand-customized pre-1.0 `## graphify` sections are detected and left untouched.

## Install just the skill (skills.sh)

```bash
npx skills add Nodesify/astria
```

The repo's canonical skill - [`skills/astria/SKILL.md`](https://github.com/Nodesify/astria/blob/main/skills/astria/SKILL.md) - is indexed on [skills.sh](https://skills.sh), so any coding agent can pick it up without astria being installed. The skill is self-contained: on first use it checks for the CLI (`astria --version`); if it is missing but `.astria/` exists, it answers from the exported report and wiki as plain files; if there is no graph either, it offers the `npm install -g @nodesify/astria` install before doing any graph work - it never installs unprompted. `astria install` (below) remains the richer path for CLI users since it also wires MCP configs, hooks, and `AGENTS.md` blocks per platform.

## Git hooks

```bash
astria hook install|uninstall|status
```

Keeps the graph fresh automatically on commit, so agents always see an up-to-date structure without anyone remembering to run `update`. Hook runs are deliberately quiet and cheap: each commit calls `update . --quiet --if-stale 10`, which prints nothing, skips the token benchmark, and does nothing at all when the graph was published less than 10 minutes ago (so a burst of commits rebuilds once, not per commit). Errors never break a commit.

Who should install them: anyone whose AI assistant queries this repo's graph — that's the workflow where staleness silently produces wrong answers. Casual CLI users can skip them without losing anything; `status` still flags a stale graph, and a deliberate `astria update .` refreshes on demand. Hooks are per-machine and per-checkout (only runs on machines where you ran `astria hook install`), and `uninstall` removes them cleanly.

## Editor guard (`hook-guard`)

```bash
astria hook-guard <mode>    # search | read | gemini
```

The editor-side companion to git hooks: a `PreToolUse` hook installed into `.claude/settings.json` that nudges agents toward `query` before raw searches. Modes:

- `search` — intercepts grep-style calls (the Grep tool, or `grep`/`rg`/`ag`/`git grep` in Bash) and injects a mandatory nudge to check the graph first
- `read` — additionally watches source-file reads: when a file is newer than the graph build, it warns that the graph is stale and to run `update` before trusting answers
- `gemini` — compatibility mode for Gemini CLI, whose `BeforeTool` hook only understands allow decisions — there the guard installs but cannot nudge

Strict mode (opt-in, via `--strict` or `ASTRIA_HOOK_STRICT=1`) additionally denies **one** un-indexed read per session until the agent orients with a graph query (`query`/`explain`/`path`); after that, reads proceed uninterrupted for `ASTRIA_HOOK_STRICT_TTL` seconds (default `1800` — 30 minutes). The guard always **fails open**: any error means the tool call proceeds untouched. See [Environment variables](../reference/env-vars#hook-guard).

## The intended loop

1. `install` once per repo — agents learn the graph exists.
2. Agents `query`/`explain`/`affected` instead of grepping (see [MCP tools reference](../reference/mcp-tools)).
3. Git hooks (or `watch`) keep the graph fresh after edits.
4. Repeated queries compound into `learned` edges — see [Memory and learning](./memory-and-learning).
