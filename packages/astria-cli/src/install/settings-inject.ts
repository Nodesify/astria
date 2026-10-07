import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { writeTextAtomic } from './atomic';
import { DEFAULT_QUERY_BUDGET } from '../defaults';

/// Strip JSONC comments (// and /* */) outside string literals, then the
/// trailing commas JSONC allows before `}` / `]`. Conservative: anything it
/// cannot cleanly strip fails JSON.parse and readJson's abort path takes
/// over — it never guesses a structure into existence.
function stripJsonc(text: string): string {
  let out = '';
  let i = 0;
  let inString = false;
  while (i < text.length) {
    const c = text[i];
    if (inString) {
      out += c;
      if (c === '\\') {
        out += text[i + 1] ?? '';
        i += 2;
        continue;
      }
      if (c === '"') inString = false;
      i += 1;
      continue;
    }
    if (c === '"') {
      inString = true;
      out += c;
      i += 1;
      continue;
    }
    if (c === '/' && text[i + 1] === '/') {
      while (i < text.length && text[i] !== '\n') i += 1;
      continue;
    }
    if (c === '/' && text[i + 1] === '*') {
      i += 2;
      while (i < text.length && !(text[i] === '*' && text[i + 1] === '/')) i += 1;
      i += 2;
      continue;
    }
    out += c;
    i += 1;
  }
  return out.replace(/,\s*([}\]])/g, '$1');
}

/// Read a JSON config the installer is about to merge into and rewrite.
/// - a UTF-8 BOM is tolerated (stripped before parsing, never written back);
/// - `jsonc: true` (VS Code's mcp.json is officially JSONC) strips comments
///   and trailing commas before parsing;
/// - an EMPTY or absent file parses as {} so a first install can create it;
/// - a non-empty file that still fails to parse ABORTS with a clear error —
///   silently continuing would overwrite the user's config with `{}` plus
///   the astria entry, destroying everything else in it.
function readJson(filePath: string, opts?: { jsonc?: boolean }): any {
  if (!fs.existsSync(filePath)) return {};
  let text = fs.readFileSync(filePath, 'utf-8');
  if (text.charCodeAt(0) === 0xfeff) text = text.slice(1);
  if (text.trim() === '') return {};
  try {
    return JSON.parse(text);
  } catch (first) {
    if (!opts?.jsonc) {
      throw new Error(
        `astria: ${filePath} is not valid JSON — refusing to rewrite it. ` +
        `Fix or remove the file manually, then re-run install. (${(first as Error).message})`
      );
    }
    try {
      return JSON.parse(stripJsonc(text));
    } catch (second) {
      throw new Error(
        `astria: ${filePath} is not valid JSONC — refusing to rewrite it. ` +
        `Fix or remove the file manually, then re-run install. (${(second as Error).message})`
      );
    }
  }
}

function writeJson(filePath: string, data: any) {
  writeTextAtomic(filePath, JSON.stringify(data, null, 2) + '\n');
}

// Hook/rule templates carry the product name, so injected entries are
// detected by substring. Current installs contain "astria"; pre-1.0
// installs contain "graphify" (never both) — inject upgrades the legacy
// entries in place and uninstall removes either era.
//
// The fingerprints are STRUCTURAL, not bare words: every hook template this
// installer has ever written embeds a quoted ".astria"/".graphify" path
// segment (or the pre-1.0 package name). A user's own hook that merely
// invokes the CLI ("astria hook-guard read --strict", a wrapper named
// my-astria-*) contains no quoted path segment and is never mistaken for a
// managed entry — installing or uninstalling astria must not touch it.
const hasCurrentFingerprint = (s: string): boolean =>
  s.includes("'.astria") || s.includes('".astria');
const hasLegacyFingerprint = (s: string): boolean =>
  s.includes("'.graphify") || s.includes('".graphify') || s.includes('nodesify-graphify');
const isCurrent = (s: string): boolean => hasCurrentFingerprint(s);
const isLegacy = (s: string): boolean => !isCurrent(s) && hasLegacyFingerprint(s);
const isAnyEra = (s: string): boolean => isCurrent(s) || hasLegacyFingerprint(s);

/// True only for MCP server entries this installer produced (either era,
/// either command shape). Uninstall deletes an entry only when this holds —
/// a user-customized or user-written `astria`/`graphify` entry is preserved,
/// the same rule inject already follows.
const isInstallerServer = (entry: any): boolean =>
  entry && typeof entry === 'object' &&
  (entry.command === 'astria'
    || entry.command === 'nodesify-graphify'
    || (Array.isArray(entry.command)
      && (entry.command[0] === 'astria' || entry.command[0] === 'nodesify-graphify')));

// ---- Claude Code (.claude/settings.json) ----

const CLAUDE_POST_UPDATE_HOOK = {
  matcher: 'Edit|Write',
  hooks: [{
    type: 'command',
    // Detached and debounced so editing never waits for a graph rebuild.
    command: `node -e "const fs=require('fs'),cp=require('child_process'),path=require('path');try{const root=cp.execSync('git rev-parse --show-toplevel',{encoding:'utf8'}).trim();const stamp=path.join(root,'.astria','.posttool-update');if(fs.existsSync(stamp)&&Date.now()-fs.statSync(stamp).mtimeMs<120000)process.exit(0);fs.mkdirSync(path.dirname(stamp),{recursive:true});fs.writeFileSync(stamp,String(Date.now()));const cli=fs.existsSync(path.join(root,'packages','astria-cli','dist','index.js'))?'node packages/astria-cli/dist/index.js update .':'npx --no-install astria update .';cp.spawn(cli,{cwd:root,shell:true,detached:true,stdio:'ignore'}).unref()}catch{}"`,
  }],
};

export function injectClaudeHook(projectDir: string): boolean {
  const settingsPath = path.join(projectDir, '.claude', 'settings.json');
  const data = readJson(settingsPath);
  if (!data.hooks) data.hooks = {};

  const pre = (data.hooks.PreToolUse || []) as any[];
  const hadLegacyPre = pre.some((h: any) => isLegacy(JSON.stringify(h.hooks)));
  // Remove pre-1.0 graphify nags when upgrading.
  data.hooks.PreToolUse = pre.filter((h: any) => !isLegacy(JSON.stringify(h.hooks)));
  if (data.hooks.PreToolUse.length === 0) delete data.hooks.PreToolUse;

  const post = (data.hooks.PostToolUse || []) as any[];
  const hadLegacyPost = post.some((h: any) => isLegacy(JSON.stringify(h.hooks)));
  const hadPost = post.some((h: any) => isCurrent(JSON.stringify(h.hooks)));
  const upgraded = post.filter((h: any) => !isLegacy(JSON.stringify(h.hooks)));
  if (!hadPost) upgraded.push(CLAUDE_POST_UPDATE_HOOK);
  data.hooks.PostToolUse = upgraded;
  if (Object.keys(data.hooks).length === 0) delete data.hooks;
  writeJson(settingsPath, data);
  return !hadPost || hadLegacyPre || hadLegacyPost;
}

export function removeClaudeHook(projectDir: string): boolean {
  const settingsPath = path.join(projectDir, '.claude', 'settings.json');
  if (!fs.existsSync(settingsPath)) return false;

  const data = readJson(settingsPath);
  if (!data.hooks) return false;

  const before = JSON.stringify(data.hooks);
  for (const event of ['PreToolUse', 'PostToolUse']) {
    if (data.hooks[event]) {
      data.hooks[event] = (data.hooks[event] as any[]).filter((h: any) =>
        !isAnyEra(JSON.stringify(h.hooks))
      );
      if (data.hooks[event].length === 0) delete data.hooks[event];
    }
  }
  if (Object.keys(data.hooks).length === 0) {
    delete data.hooks;
  }
  writeJson(settingsPath, data);
  return JSON.stringify(data.hooks) !== before;
}

// ---- Codex (.codex/hooks.json) ----

const CODEX_HOOK_COMMAND = `node -e "const fs=require('fs');const p='.astria/graph.json';if(!fs.existsSync(p)){process.exit(0)}const msg='astria: Knowledge graph available. Use astria query for architecture questions. Read .astria/graph_report.md first.';process.stdout.write(JSON.stringify({hookSpecificOutput:{hookEventName:'PreToolUse',additionalContext:msg}}))"`;

export function injectCodexHook(projectDir: string): boolean {
  const hooksPath = path.join(projectDir, '.codex', 'hooks.json');
  const data = readJson(hooksPath);
  if (!data.hooks) data.hooks = {};

  const existing = (data.hooks.PreToolUse || []) as any[];
  const hadLegacy = existing.some((h: any) => isLegacy(JSON.stringify(h.hooks || [])));
  const current = existing.filter((h: any) => !isLegacy(JSON.stringify(h.hooks || [])));
  const alreadyExists = current.some((h: any) => isCurrent(JSON.stringify(h.hooks || [])));
  if (alreadyExists && !hadLegacy) return false;
  if (alreadyExists) {
    data.hooks.PreToolUse = current;
    writeJson(hooksPath, data);
    return true;
  }

  current.push({
    matcher: 'Bash',
    hooks: [{
      type: 'command',
      command: CODEX_HOOK_COMMAND,
    }],
  });
  data.hooks.PreToolUse = current;
  writeJson(hooksPath, data);
  return true;
}

export function removeCodexHook(projectDir: string): boolean {
  const hooksPath = path.join(projectDir, '.codex', 'hooks.json');
  if (!fs.existsSync(hooksPath)) return false;

  const data = readJson(hooksPath);
  if (!data.hooks?.PreToolUse) return false;

  const before = (data.hooks.PreToolUse as any[]).length;
  data.hooks.PreToolUse = (data.hooks.PreToolUse as any[]).filter((h: any) =>
    !isAnyEra(JSON.stringify(h.hooks || []))
  );
  writeJson(hooksPath, data);
  return (data.hooks.PreToolUse as any[]).length !== before;
}

// ---- Gemini (.gemini/settings.json) ----

const GEMINI_HOOK_COMMAND = `node -e "const fs=require('fs');const p='.astria/graph.json';var r={decision:'allow'};if(fs.existsSync(p)){r.additionalContext='astria: Knowledge graph available. Use astria query for architecture questions. Read .astria/graph_report.md first.'}process.stdout.write(JSON.stringify(r))"`;

export function injectGeminiHook(projectDir: string): boolean {
  const settingsPath = path.join(projectDir, '.gemini', 'settings.json');
  const data = readJson(settingsPath);
  if (!data.hooks) data.hooks = {};

  const existing = (data.hooks.BeforeTool || []) as any[];
  const hadLegacy = existing.some(
    (h: any) =>
      h.matcher === 'read_file|list_directory' && isLegacy(JSON.stringify(h.hooks || []))
  );
  const current = existing.filter(
    (h: any) => !(h.matcher === 'read_file|list_directory' && isLegacy(JSON.stringify(h.hooks || [])))
  );
  const alreadyExists = current.some(
    (h: any) =>
      h.matcher === 'read_file|list_directory' && isCurrent(JSON.stringify(h.hooks || []))
  );
  if (alreadyExists && !hadLegacy) return false;

  if (!alreadyExists) {
    current.push({
      matcher: 'read_file|list_directory',
      hooks: [{
        type: 'command',
        command: GEMINI_HOOK_COMMAND,
      }],
    });
  }
  data.hooks.BeforeTool = current;
  writeJson(settingsPath, data);
  return true;
}

export function removeGeminiHook(projectDir: string): boolean {
  const settingsPath = path.join(projectDir, '.gemini', 'settings.json');
  if (!fs.existsSync(settingsPath)) return false;

  const data = readJson(settingsPath);
  if (!data.hooks?.BeforeTool) return false;

  const before = (data.hooks.BeforeTool as any[]).length;
  data.hooks.BeforeTool = (data.hooks.BeforeTool as any[]).filter((h: any) =>
    !isAnyEra(JSON.stringify(h.hooks || []))
  );
  writeJson(settingsPath, data);
  return (data.hooks.BeforeTool as any[]).length !== before;
}

// ---- OpenCode (.opencode/) ----

const OPENCODE_PLUGIN_JS = `// astria OpenCode plugin
import { existsSync } from "fs";
import { join } from "path";

export const AstriaPlugin = async ({ directory }) => {
  const reminded = new Set();
  return {
    "tool.execute.before": async (input, output) => {
      if (reminded.has(input.tool)) return;
      if (!["view", "grep", "glob", "ls", "bash"].includes(input.tool)) return;
      if (!existsSync(join(directory, ".astria", "graph.json"))) return;
      if (input.tool === "bash") {
        output.args.command =
          'echo "[astria] Knowledge graph available. MUST read .astria/graph_report.md before searching raw files. Use astria query instead of grep for architecture questions." && ' +
          output.args.command;
      } else {
        output.error = new Error(
          "[astria] Knowledge graph available. MUST read .astria/graph_report.md before searching raw files. Use astria query instead of grep for architecture questions."
        );
      }
      reminded.add(input.tool);
    },
  };
};
`;

export function injectOpenCodePlugin(projectDir: string): boolean {
  // OpenCode auto-discovers plugins from `.opencode/plugins/` — the current
  // documented convention (opencode.ai/docs/plugins). Verified against
  // opencode 1.17.8 locally: `opencode debug config` lists plugin files from
  // both the plural and the singular directory, so writing `plugins/` works
  // there too. opencode 1.17+ REJECTS a `plugins` key in opencode.json
  // ("Unrecognized key"), so the plugin is a file drop with no config
  // registration. Existing user configuration is retained.
  const pluginDir = path.join(projectDir, '.opencode', 'plugins');
  const pluginPath = path.join(pluginDir, 'astria.js');
  const legacyDir = path.join(projectDir, '.opencode', 'plugin');
  const legacyPaths = [
    // 1.0.9-era singular directory, both names.
    path.join(legacyDir, 'graphify.js'),
    path.join(legacyDir, 'astria.js'),
    // pre-1.0.9 wrote the old graphify name into the plural directory.
    path.join(pluginDir, 'graphify.js'),
  ];

  let changed = false;
  for (const legacy of legacyPaths) {
    if (fs.existsSync(legacy)) {
      fs.unlinkSync(legacy);
      changed = true;
    }
  }
  if (fs.existsSync(pluginPath) && fs.readFileSync(pluginPath, 'utf8') === OPENCODE_PLUGIN_JS) return changed;
  if (!fs.existsSync(pluginDir)) {
    fs.mkdirSync(pluginDir, { recursive: true });
  }
  writeTextAtomic(pluginPath, OPENCODE_PLUGIN_JS);
  return true;
}

export function removeOpenCodePlugin(projectDir: string): boolean {
  let changed = false;
  for (const dirName of ['plugin', 'plugins']) {
    for (const name of ['astria.js', 'graphify.js']) {
      const pluginPath = path.join(projectDir, '.opencode', dirName, name);
      if (fs.existsSync(pluginPath)) {
        fs.unlinkSync(pluginPath);
        changed = true;
      }
    }
  }
  return changed;
}

// ---- Cursor (.cursor/rules/astria.mdc) ----

const CURSOR_RULE = `---
description: astria knowledge graph context
alwaysApply: true
---

This project has an astria knowledge graph at .astria/.

Rules:
- MUST read .astria/graph_report.md before searching files for architecture or codebase questions
- MUST use \`astria query "<question>"\`, \`astria path "<A>" "<B>"\`, or \`astria explain "<concept>"\` for cross-module questions — do NOT grep/read files directly for these
- After modifying code files, run \`astria update .\` to keep the graph current
`;

export function injectCursorRule(projectDir: string): boolean {
  const ruleDir = path.join(projectDir, '.cursor', 'rules');
  const rulePath = path.join(ruleDir, 'astria.mdc');
  // Pre-1.0 installs wrote graphify.mdc — drop it on upgrade.
  const legacyPath = path.join(ruleDir, 'graphify.mdc');
  if (fs.existsSync(legacyPath)) fs.unlinkSync(legacyPath);

  if (!fs.existsSync(ruleDir)) {
    fs.mkdirSync(ruleDir, { recursive: true });
  }
  writeTextAtomic(rulePath, CURSOR_RULE);
  return true;
}

export function removeCursorRule(projectDir: string): boolean {
  let changed = false;
  for (const name of ['astria.mdc', 'graphify.mdc']) {
    const rulePath = path.join(projectDir, '.cursor', 'rules', name);
    if (fs.existsSync(rulePath)) {
      fs.unlinkSync(rulePath);
      changed = true;
    }
  }
  return changed;
}

// ---- Project-scoped MCP registration ----
// Codex is the exception: its MCP servers live in the user-global
// ~/.codex/config.toml (TOML), handled by the dedicated branch below.

export type McpFlavor =
  | 'zcode'
  | 'claude'
  | 'cursor'
  | 'gemini'
  | 'vscode'
  | 'trae'
  | 'windsurf'
  | 'kiro'
  | 'opencode'
  | 'pi'
  | 'codex';

const ASTRIA_MCP_SERVER = { type: 'stdio', command: 'astria', args: ['mcp'] };

// Server registration key per flavor, plus an optional vendor-specific
// server object (OpenCode's schema differs: `type: "local"`, command array,
// explicit `enabled` — verified against opencode 1.17.8's config validator).
// The legacy "graphify" key is matched for upgrade (inject) and cleanup
// (remove). 'codex' has no JSON target — injectAgentMcp dispatches it to
// the TOML writer.
const MCP_TARGETS: Partial<Record<McpFlavor, {
  configPath: string;
  serverPath: string[];
  server?: Record<string, unknown>;
  /** Pre-1.0.9 server location this installer wrote; cleaned on inject and
   * remove (only entries this installer produced — a customized entry with
   * a foreign command is preserved like every other legacy case). */
  legacyServerPath?: string[];
  /** Whole config files a previous release wrote at a since-corrected path;
   * their astria entry is cleaned on inject and remove (file deleted when
   * nothing of the user's remains). */
  legacyConfigPaths?: string[];
}>> = {
  zcode: { configPath: path.join('.zcode', 'config.json'), serverPath: ['mcp', 'servers', 'astria'] },
  claude: { configPath: '.mcp.json', serverPath: ['mcpServers', 'astria'] },
  cursor: { configPath: path.join('.cursor', 'mcp.json'), serverPath: ['mcpServers', 'astria'] },
  gemini: { configPath: path.join('.gemini', 'settings.json'), serverPath: ['mcpServers', 'astria'] },
  // VS Code native workspace MCP (also what Copilot inside VS Code uses).
  // Shape matches VS Code 1.137's own `--add-mcp` writer exactly: a
  // `servers` map with bare command/args (no type field). The 1.0.8-era
  // `mcpServers` key this installer wrote is migrated away on inject.
  vscode: {
    configPath: path.join('.vscode', 'mcp.json'),
    serverPath: ['servers', 'astria'],
    server: { command: 'astria', args: ['mcp'] },
    legacyServerPath: ['mcpServers', 'astria'],
  },
  trae: { configPath: path.join('.trae', 'mcp.json'), serverPath: ['mcpServers', 'astria'] },
  windsurf: { configPath: path.join('.windsurf', 'mcp.json'), serverPath: ['mcpServers', 'astria'] },
  // Kiro workspace MCP servers live in .kiro/settings/mcp.json (kiro.dev docs:
  // workspace scope, `mcpServers` key — verified 2026-09-30). The 1.0.9-era
  // bare mcp.json at the project root this installer wrote is a path nothing
  // reads; it is cleaned up on inject and remove.
  kiro: {
    configPath: path.join('.kiro', 'settings', 'mcp.json'),
    serverPath: ['mcpServers', 'astria'],
    legacyConfigPaths: ['mcp.json'],
  },
  // OpenCode keys servers directly under `mcp` (no `servers` intermediate)
  // and requires its own server shape. The 1.0.8-era nested path is
  // migrated away on inject.
  opencode: {
    configPath: path.join('.opencode', 'opencode.json'),
    serverPath: ['mcp', 'astria'],
    server: { type: 'local', command: ['astria', 'mcp'], enabled: true },
    legacyServerPath: ['mcp', 'servers', 'astria'],
  },
  // Pi reads the standard .mcp.json via the pi-mcp-adapter extension —
  // same file as claude; its own flavor so install output stays honest.
  pi: { configPath: '.mcp.json', serverPath: ['mcpServers', 'astria'] },
};

export function inspectAgentMcp(root: string, flavor: McpFlavor): { file: string; present: boolean; managed: boolean } {
  if (flavor === 'codex') {
    const file = codexConfigPath(root);
    let text: string;
    try { text = fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n'); }
    catch (error: any) { if (error.code === 'ENOENT') return { file, present: false, managed: false }; throw error; }
    return { file, present: /^\s*\[mcp_servers\.astria\]\s*$/m.test(text), managed: text.includes(CODEX_MCP_BLOCK) };
  }
  const target = MCP_TARGETS[flavor]!;
  const file = path.join(root, target.configPath);
  const data = readJson(file, { jsonc: flavor === 'vscode' });
  const entry = target.serverPath.reduce((node: any, key: string) => node?.[key], data);
  return { file, present: !!entry, managed: isInstallerServer(entry) };
}
const LEGACY_MCP_SERVER_NAME = 'graphify';

// ---- Copilot (.github/copilot-mcp.json) ----

// The Copilot coding agent has no committed repo file for MCP servers:
// repository-level configuration is entered as JSON in the repository
// Settings UI (docs.github.com, "Configure MCP servers for your repository").
// The 1.0.9-era .github/copilot-mcp.json this installer wrote is read by
// nothing; it is removed on install and uninstall when it only carries the
// astria entry this installer produced — a file with other content stays.
export function cleanupLegacyCopilotMcp(projectDir: string): boolean {
  const legacyPath = path.join(projectDir, '.github', 'copilot-mcp.json');
  if (!fs.existsSync(legacyPath)) return false;
  const data = readJson(legacyPath);
  let found = false;
  for (const container of ['servers', 'mcpServers']) {
    const node = data[container];
    if (typeof node !== 'object' || node === null) continue;
    const names = Object.keys(node).filter((n) => n === 'astria' || n === LEGACY_MCP_SERVER_NAME);
    if (!names.every((n) => isInstallerServer(node[n]))) continue;
    for (const n of names) delete node[n];
    if (Object.keys(node).length === 0) delete data[container];
    found = found || names.length > 0;
  }
  if (!found) return false;
  if (Object.keys(data).length === 0) {
    fs.unlinkSync(legacyPath);
  } else {
    writeJson(legacyPath, data);
  }
  return true;
}

/// Removes astria entries this installer wrote in legacy config *files*
/// (whole files at since-corrected paths, e.g. Kiro's 1.0.9 root mcp.json).
/// Only installer-produced entries (command astria / nodesify-graphify) are
/// deleted; empty containers are pruned and a file left as `{}` is removed.
/// `keepPath` skips the flavor's current target file. Returns whether
/// anything changed.
function cleanupLegacyConfigPaths(
  projectDir: string,
  target: { serverPath: string[]; legacyConfigPaths?: string[] },
  keepPath: string
): boolean {
  if (!target.legacyConfigPaths) return false;
  let changed = false;
  for (const legacyRel of target.legacyConfigPaths) {
    const legacyPath = path.join(projectDir, legacyRel);
    if (legacyPath === keepPath || !fs.existsSync(legacyPath)) continue;
    const data = readJson(legacyPath);
    let node: any = data;
    const parents: Array<[any, string]> = [];
    let reachable = true;
    for (const key of target.serverPath.slice(0, -1)) {
      if (typeof node[key] !== 'object' || node[key] === null) {
        reachable = false;
        break;
      }
      parents.push([node, key]);
      node = node[key];
    }
    if (!reachable) continue;
    const name = target.serverPath[target.serverPath.length - 1];
    if (!isInstallerServer(node[name]) && !isInstallerServer(node[LEGACY_MCP_SERVER_NAME])) continue;
    delete node[name];
    delete node[LEGACY_MCP_SERVER_NAME];
    for (let i = parents.length - 1; i >= 0; i--) {
      const [parent, key] = parents[i];
      if (Object.keys(parent[key]).length === 0) delete parent[key];
    }
    if (Object.keys(data).length === 0) {
      fs.unlinkSync(legacyPath);
    } else {
      writeJson(legacyPath, data);
    }
    changed = true;
  }
  return changed;
}

// ---- Codex MCP (~/.codex/config.toml, user-global TOML) ----

// The managed table is deterministic so removal can verify it is ours and
// never touch a hand-written [mcp_servers.astria] section.
const CODEX_MCP_BLOCK = '[mcp_servers.astria]\ncommand = "astria"\nargs = ["mcp"]';

function codexConfigPath(root = os.homedir()): string {
  return path.join(root, '.codex', 'config.toml');
}

export function injectCodexMcp(root?: string): boolean {
  const configPath = codexConfigPath(root);
  let text = '';
  try {
    text = fs.readFileSync(configPath, 'utf8');
  } catch (error: any) {
    if (error.code !== 'ENOENT') throw error;
  }
  const hasSection = text
    .split('\n')
    .some((line) => line.trim() === '[mcp_servers.astria]');
  if (hasSection) {
    // Ours (the exact managed block) or the user's own definition — either
    // way it is present and must not be duplicated or clobbered.
    return false;
  }
  const next = (text.length > 0 && !text.endsWith('\n') ? text + '\n' : text) + CODEX_MCP_BLOCK + '\n';
  writeTextAtomic(configPath, next);
  return true;
}

export function removeCodexMcp(root?: string): boolean {
  const configPath = codexConfigPath(root);
  let text: string;
  try {
    text = fs.readFileSync(configPath, 'utf8');
  } catch (error: any) {
    if (error.code !== 'ENOENT') throw error;
    return false;
  }
  const lines = text.split('\n');
  const headerIdx = lines.findIndex((line) => line.trim() === '[mcp_servers.astria]');
  if (headerIdx === -1) return false;

  // The section body runs to the next TOML table header (or EOF).
  let end = headerIdx + 1;
  while (end < lines.length && !lines[end].trimStart().startsWith('[')) {
    end++;
  }
  const body = lines.slice(headerIdx + 1, end).join('\n').trim();
  const managed = CODEX_MCP_BLOCK.split('\n').slice(1).join('\n').trim();
  if (body !== managed) {
    // User-customized — leave it alone, same rule as the JSON legacy entry.
    return false;
  }
  lines.splice(headerIdx, end - headerIdx);
  let next = lines.join('\n');
  // Drop the blank line the injection left before the block, if any.
  next = next.replace(/\n\n+$/, '\n');
  writeTextAtomic(configPath, next);
  return true;
}

export function injectAgentMcp(projectDir: string, flavor: McpFlavor): boolean {
  if (flavor === 'codex') {
    return injectCodexMcp(projectDir);
  }
  const target = MCP_TARGETS[flavor]!;
  const configPath = path.join(projectDir, target.configPath);
  // Config files a previous release wrote at since-corrected paths are
  // cleaned up whether or not the current target ends up being written.
  const removedLegacyFile = cleanupLegacyConfigPaths(projectDir, target, configPath);
  // VS Code's mcp.json is officially JSONC — comments and trailing commas
  // are legal there. Comments do not survive the rewrite (the merged file
  // is written as plain JSON, which JSONC parsers accept).
  const data = readJson(configPath, { jsonc: flavor === 'vscode' });

  let node: any = data;
  for (const key of target.serverPath.slice(0, -1)) {
    if (typeof node[key] !== 'object' || node[key] === null) node[key] = {};
    node = node[key];
  }
  const name = target.serverPath[target.serverPath.length - 1];

  // A pre-1.0 server entry under the legacy name written by this installer
  // is replaced; a user-customized legacy entry (different command) is left.
  const legacy = node[LEGACY_MCP_SERVER_NAME];
  let removedLegacy = false;
  if (legacy && legacy.command === 'nodesify-graphify') {
    delete node[LEGACY_MCP_SERVER_NAME];
    removedLegacy = true;
  }

  // A server this installer wrote at a since-moved path is migrated away,
  // not duplicated — opencode rejects the old nested location outright.
  // `node` guard: pruning the old parent chain never orphans the new entry.
  if (migrateLegacyServerPath(data, target, node)) removedLegacy = true;

  if (node[name]) {
    if (removedLegacy) {
      writeJson(configPath, data);
      return true;
    }
    return removedLegacyFile;
  }

  node[name] = { ...(target.server ?? ASTRIA_MCP_SERVER) };
  migrateLegacyServerPath(data, target, node);
  writeJson(configPath, data);
  return true;
}

/// Removes a server entry this installer wrote at a since-moved path
/// (`legacyServerPath`), pruning its now-empty parents. Only installer-
/// produced entries (command astria / nodesify-graphify) are touched;
/// returns whether anything changed.
function migrateLegacyServerPath(
  data: any,
  target: { legacyServerPath?: string[] },
  keepNode: any
): boolean {
  if (!target.legacyServerPath) return false;
  let legacyNode: any = data;
  const legacyParents: Array<[any, string]> = [];
  for (const key of target.legacyServerPath.slice(0, -1)) {
    if (typeof legacyNode[key] !== 'object' || legacyNode[key] === null) {
      legacyNode = null;
      break;
    }
    legacyParents.push([legacyNode, key]);
    legacyNode = legacyNode[key];
  }
  if (!legacyNode) return false;
  const legacyName = target.legacyServerPath[target.legacyServerPath.length - 1];
  const legacyEntry = legacyNode[legacyName] ?? legacyNode[LEGACY_MCP_SERVER_NAME];
  const isOurs = legacyEntry && (legacyEntry.command === 'astria'
    || (Array.isArray(legacyEntry.command) && legacyEntry.command[0] === 'astria')
    || legacyEntry.command === 'nodesify-graphify');
  if (!isOurs) return false;
  delete legacyNode[legacyName];
  delete legacyNode[LEGACY_MCP_SERVER_NAME];
  for (let i = legacyParents.length - 1; i >= 0; i--) {
    const [parent, key] = legacyParents[i];
    if (parent[key] !== keepNode && Object.keys(parent[key]).length === 0) {
      delete parent[key];
    }
  }
  return true;
}

export function removeAgentMcp(projectDir: string, flavor: McpFlavor): boolean {
  if (flavor === 'codex') {
    return removeCodexMcp(projectDir);
  }
  const target = MCP_TARGETS[flavor]!;
  const configPath = path.join(projectDir, target.configPath);
  // Legacy config files are cleaned even when the current target is absent
  // (an install made before a path correction, then a straight uninstall).
  const removedLegacyFile = cleanupLegacyConfigPaths(projectDir, target, configPath);
  if (!fs.existsSync(configPath)) return removedLegacyFile;

  const data = readJson(configPath, { jsonc: flavor === 'vscode' });
  const parents: Array<[any, string]> = [];
  let node: any = data;
  for (const key of target.serverPath.slice(0, -1)) {
    if (typeof node[key] !== 'object' || node[key] === null) return false;
    parents.push([node, key]);
    node = node[key];
  }
  const name = target.serverPath[target.serverPath.length - 1];

  // Only entries this installer wrote are removed — the same rule inject
  // follows. A user-customized or user-written `astria`/`graphify` entry
  // survives uninstall.
  let removed = false;
  if (isInstallerServer(node[name])) {
    delete node[name];
    removed = true;
  }
  if (isInstallerServer(node[LEGACY_MCP_SERVER_NAME])) {
    delete node[LEGACY_MCP_SERVER_NAME];
    removed = true;
  }
  if (target.legacyServerPath) {
    let legacyNode: any = data;
    const legacyParents: Array<[any, string]> = [];
    for (const key of target.legacyServerPath.slice(0, -1)) {
      if (typeof legacyNode[key] !== 'object' || legacyNode[key] === null) {
        legacyNode = null;
        break;
      }
      legacyParents.push([legacyNode, key]);
      legacyNode = legacyNode[key];
    }
    if (legacyNode) {
      const legacyName = target.legacyServerPath[target.legacyServerPath.length - 1];
      if (isInstallerServer(legacyNode[legacyName])) {
        delete legacyNode[legacyName];
        removed = true;
      }
      if (isInstallerServer(legacyNode[LEGACY_MCP_SERVER_NAME])) {
        delete legacyNode[LEGACY_MCP_SERVER_NAME];
        removed = true;
      }
      for (let i = legacyParents.length - 1; i >= 0; i--) {
        const [parent, key] = legacyParents[i];
        if (parent[key] !== undefined && Object.keys(parent[key]).length === 0) delete parent[key];
      }
    }
  }
  if (!removed) return removedLegacyFile;
  for (let i = parents.length - 1; i >= 0; i--) {
    const [parent, key] = parents[i];
    if (parent[key] !== undefined && Object.keys(parent[key]).length === 0) delete parent[key];
  }
  writeJson(configPath, data);
  return true;
}

export function injectZcodeMcp(projectDir: string): boolean {
  return injectAgentMcp(projectDir, 'zcode');
}

export function removeZcodeMcp(projectDir: string): boolean {
  return removeAgentMcp(projectDir, 'zcode');
}

// ---- Pi (~/.pi/agent/extensions/astria.mjs) ----

// Pi auto-discovers extensions from ~/.pi/agent/extensions/; entry shape is
// `export default function (pi)`. The API surface here (registerTool,
// tool_result, registerCommand) was read from pi 0.87's ExtensionAPI types.
// Native tools are the pi-idiomatic integration — pi's own philosophy is
// CLI tools over MCP definitions (a registered tool costs a few hundred
// context tokens; ten MCP tool definitions cost 10k+), so the graph is
// exposed as first-class pi tools backed by the astria CLI. The standard
// .mcp.json registration stays for users of the pi-mcp-adapter extension.
const PI_EXTENSION_JS = `// astria extension for the Pi coding agent.
// Native tools (pi-idiomatic): astria_query/map/explain/path/affected,
// backed by the astria CLI. Freshness: write/edit refresh the graph
// (throttled, detached). /astria: guidance.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";

function astriaCli(cwd) {
  return existsSync(join(cwd, "packages", "astria-cli", "dist", "index.js"))
    ? "node packages/astria-cli/dist/index.js"
    : "astria";
}

function run(args, cwd) {
  if (!existsSync(join(cwd, ".astria"))) {
    return Promise.resolve({
      out: 'No astria graph in this project. Build one first: run "astria run ." (creates .astria/ with the knowledge graph).',
    });
  }
  return new Promise((resolve) => {
    const child = spawn(astriaCli(cwd) + " " + args, { cwd, shell: true });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill(), 90000);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("error", (e) => { clearTimeout(timer); resolve({ out: String(e) }); });
    child.on("close", () => { clearTimeout(timer); resolve({ out: out || err }); });
  });
}

function textResult(text, command) {
  return { content: [{ type: "text", text: String(text) }], details: { command } };
}

function schema(props, required) {
  return { type: "object", properties: props, required };
}

let lastUpdate = 0;
function refreshGraph(cwd, awaitSpawn) {
  try {
    const now = Date.now();
    if (now - lastUpdate < 120000) return Promise.resolve(false);
    lastUpdate = now;
    const child = spawn(astriaCli(cwd) + " update .", { cwd, shell: true, detached: true, stdio: "ignore" });
    child.unref();
    if (!awaitSpawn) return Promise.resolve(true);
    // Print-mode sessions exit the instant the tool returns — a detached
    // child whose spawn syscall has not completed yet never materializes.
    // Wait until the OS has actually created it (bounded) before returning.
    return new Promise((resolve) => {
      const done = () => resolve(true);
      child.once("spawn", done);
      setTimeout(done, 1000);
    });
  } catch {
    return Promise.resolve(false);
  }
}

const ASTRIA_TOOLS = [
  {
    name: "astria_query",
    cli: "query",
    args: (p) => "query " + JSON.stringify(p.question) + " --budget " + (p.budget || DEFAULT_QUERY_BUDGET) + " --graph .",
    description: 'Query the repo knowledge graph with a natural-language question. Ranked nodes carry file:line anchors and edge provenance. Prefer over grep for architecture/cross-module questions. "No confident match" means the vocabulary is absent — rephrase toward symbol names.',
    props: {
      question: { type: "string", description: "Natural-language question about the codebase" },
      budget: { type: "integer", description: "Max output tokens (default " + DEFAULT_QUERY_BUDGET + ")" },
    },
    required: ["question"],
  },
  {
    name: "astria_map",
    cli: "map",
    args: () => "map --budget " + DEFAULT_QUERY_BUDGET + " --graph .",
    description: "PageRank-ranked repo map with top symbols per file — orient before diving in.",
    props: {},
    required: [],
  },
  {
    name: "astria_explain",
    cli: "explain",
    args: (p) => "explain " + JSON.stringify(p.node) + " --graph .",
    description: "Explain a symbol: metadata plus its strongest connections with real edge direction and evidence tier.",
    props: { node: { type: "string", description: "Symbol or node name" } },
    required: ["node"],
  },
  {
    name: "astria_path",
    cli: "path",
    args: (p) => "path " + JSON.stringify(p.from) + " " + JSON.stringify(p.to) + " --graph .",
    description: "Shortest connection path between two symbols or concepts, relations per hop.",
    props: { from: { type: "string" }, to: { type: "string" } },
    required: ["from", "to"],
  },
  {
    name: "astria_affected",
    cli: "affected",
    args: (p) => "affected " + JSON.stringify(p.node) + " --depth " + (p.depth || 2) + " --graph .",
    description: "Blast radius — run BEFORE changing a shared symbol. Hops show evidence tier (RESOLVED = source-extracted call uniquely bound; INFERRED = weaker).",
    props: {
      node: { type: "string", description: "Symbol to assess" },
      depth: { type: "integer", description: "Max hops (default 2)" },
    },
    required: ["node"],
  },
];

export default function (pi) {
  for (const t of ASTRIA_TOOLS) {
    try {
      pi.registerTool({
        name: t.name,
        label: t.name,
        description: t.description,
        parameters: schema(t.props, t.required),
        async execute(toolCallId, params, signal, onUpdate, ctx) {
          const cwd = (ctx && ctx.cwd) || (typeof process !== "undefined" ? process.cwd() : ".");
          const out = await run(t.args(params || {}), cwd);
          // Mode-independent freshness: pi 0.87 print-mode sessions do not
          // deliver tool_result events to extensions (measured), so every
          // graph tool call also kicks the throttled refresh — awaited
          // until the update process exists, because the session exits
          // the moment this tool returns. The staleness disclosure in the
          // output covers the gap until the next query.
          await refreshGraph(cwd, true);
          return textResult(out.out || "(no output)", "astria " + t.cli);
        },
      });
    } catch { /* one failed registration must not block the rest */ }
  }

  pi.on("tool_result", (event, ctx) => {
    const tool = event && (event.tool ?? event.toolName);
    if (tool !== "write" && tool !== "edit") return;
    refreshGraph((ctx && ctx.cwd) || (typeof process !== "undefined" ? process.cwd() : "."));
  });

  try {
    pi.registerCommand("astria", {
      description: "astria knowledge-graph guidance for this repo",
      handler: async (args, ctx) => {
        const lines = [
          "astria knowledge graph:",
          "- .astria/ holds a queryable graph of this repo (built by 'astria run').",
          "- Native tools available: astria_query, astria_map, astria_explain,",
          "  astria_path, astria_affected — prefer them over grep for",
          "  architecture and cross-module questions.",
          "- Output carries file:line anchors and edge provenance",
          '  (EXTRACTED/RESOLVED/INFERRED); "No confident match" means rephrase',
          "  toward symbol names or file paths.",
          "- After edits the graph refreshes automatically (throttled);",
          "  'astria update .' refreshes on demand.",
        ];
        const msg = lines.join("\\n");
        try { pi.sendToolResult({ title: "astria", data: msg }); }
        catch { console.log(msg); }
      },
    });
  } catch { /* command registration is optional polish */ }
}
`;

export function injectPiExtension(projectDir?: string): boolean {
  const dir = projectDir ? path.join(projectDir, '.pi', 'extensions') : path.join(os.homedir(), '.pi', 'agent', 'extensions');
  const extPath = path.join(dir, 'astria.mjs');
  if (fs.existsSync(extPath)) {
    const current = fs.readFileSync(extPath, 'utf-8');
    if (current === PI_EXTENSION_JS) return false;
  }
  fs.mkdirSync(dir, { recursive: true });
  writeTextAtomic(extPath, PI_EXTENSION_JS);
  return true;
}

export function removePiExtension(projectDir?: string): boolean {
  const extPath = projectDir ? path.join(projectDir, '.pi', 'extensions', 'astria.mjs') : path.join(os.homedir(), '.pi', 'agent', 'extensions', 'astria.mjs');
  if (!fs.existsSync(extPath)) return false;
  fs.unlinkSync(extPath);
  return true;
}

/** Standalone files are wholly owned; merged configs are handled separately. */
export function generatedIntegrationFiles(root: string, platform: string, user: boolean): Record<string, string> {
  if (platform === 'pi') return { [path.join(root, '.pi', ...(user ? ['agent'] : []), 'extensions', 'astria.mjs')]: PI_EXTENSION_JS };
  if (user) return {};
  if (platform === 'cursor') return { [path.join(root, '.cursor', 'rules', 'astria.mdc')]: CURSOR_RULE };
  if (platform === 'kiro') return { [path.join(root, '.kiro', 'steering', 'astria.md')]: KIRO_STEERING };
  if (platform === 'opencode') return { [path.join(root, '.opencode', 'plugins', 'astria.js')]: OPENCODE_PLUGIN_JS };
  return {};
}

// ---- Kiro (.kiro/steering/astria.md) ----

const KIRO_STEERING = `---
inclusion: always
---

astria: A knowledge graph of this project lives in \`.astria/\`.

Rules:
- MUST read \`.astria/graph_report.md\` before searching files for architecture or codebase questions
- MUST use \`astria query\`, \`astria path\`, or \`astria explain\` for cross-module questions — do NOT grep/read files directly
- After modifying code files, run \`astria update .\` to keep the graph current
`;

export function injectKiroSteering(projectDir: string): boolean {
  const steerDir = path.join(projectDir, '.kiro', 'steering');
  const steerPath = path.join(steerDir, 'astria.md');
  // Pre-1.0 installs wrote graphify.md — drop it on upgrade.
  const legacyPath = path.join(steerDir, 'graphify.md');
  if (fs.existsSync(legacyPath)) fs.unlinkSync(legacyPath);

  if (!fs.existsSync(steerDir)) {
    fs.mkdirSync(steerDir, { recursive: true });
  }
  writeTextAtomic(steerPath, KIRO_STEERING);
  return true;
}

export function removeKiroSteering(projectDir: string): boolean {
  let changed = false;
  for (const name of ['astria.md', 'graphify.md']) {
    const steerPath = path.join(projectDir, '.kiro', 'steering', name);
    if (fs.existsSync(steerPath)) {
      fs.unlinkSync(steerPath);
      changed = true;
    }
  }
  return changed;
}
