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
nodesify-graphify mcp [--graph .]
```

Runs the MCP stdio server — nine tools for querying the graph from any MCP-capable agent. The full tool list, arguments, and example calls are in the [MCP tools reference](../reference/mcp-tools). Add it to your agent's MCP config:

```json
{
  "mcpServers": {
    "graphify": { "command": "nodesify-graphify", "args": ["mcp"] }
  }
}
```

## Skill files (`install`)

```bash
nodesify-graphify install [--platform claude]
nodesify-graphify uninstall [--platform claude]
```

Supported platforms: `claude`, `codex`, `gemini`, `cursor`, `copilot`, `aider`, `opencode`, `kiro`, `trae`, `zcode`.

`install` writes the platform's skill files and injects an always-on `## graphify` instruction block into `AGENTS.md` / `CLAUDE.md` — telling agents to query the graph before grepping and to run `update` after edits. The instruction block names both access paths: MCP tools when the `graphify` server is connected, or the `nodesify-graphify` CLI from any agent.

`--platform zcode` additionally registers the graphify MCP server in `.zcode/config.json` under `mcp.servers`, which ZCode trusts and auto-connects at session start — the tools (`repo_map`, `query_graph`, `explain`, `get_neighbors`, `shortest_path`, `affected`) appear natively in every session for that project. All steps are idempotent; `uninstall` removes them.

## Git hooks

```bash
nodesify-graphify hook install|uninstall|status
```

Keeps the graph fresh automatically on commit, so agents always see an up-to-date structure without anyone remembering to run `update`.

## Editor guard (`hook-guard`)

```bash
nodesify-graphify hook-guard <mode>    # search | read | gemini
```

The editor-side companion to git hooks: a `PreToolUse` hook installed into `.claude/settings.json` that nudges agents toward `query` before raw searches. Modes:

- `search` — intercepts grep-style calls (the Grep tool, or `grep`/`rg`/`ag`/`git grep` in Bash) and injects a mandatory nudge to check the graph first
- `read` — additionally watches source-file reads: when a file is newer than the graph build, it warns that the graph is stale and to run `update` before trusting answers
- `gemini` — compatibility mode for Gemini CLI, whose `BeforeTool` hook only understands allow decisions — there the guard installs but cannot nudge

Strict mode (opt-in, via `--strict` or `GRAPHIFY_HOOK_STRICT=1`) additionally denies **one** un-indexed read per session until the agent orients with a graph query (`query`/`explain`/`path`); after that, reads proceed uninterrupted for `GRAPHIFY_HOOK_STRICT_TTL` seconds (default `1800` — 30 minutes). The guard always **fails open**: any error means the tool call proceeds untouched. See [Environment variables](../reference/env-vars#hook-guard).

## The intended loop

1. `install` once per repo — agents learn the graph exists.
2. Agents `query`/`explain`/`affected` instead of grepping (see [MCP tools reference](../reference/mcp-tools)).
3. Git hooks (or `watch`) keep the graph fresh after edits.
4. Repeated queries compound into `learned` edges — see [Memory and learning](./memory-and-learning).
