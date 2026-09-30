import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

function readJson(filePath: string): any {
  if (!fs.existsSync(filePath)) return {};
  try {
    return JSON.parse(fs.readFileSync(filePath, 'utf-8'));
  } catch {
    return {};
  }
}

function writeJson(filePath: string, data: any) {
  const dir = path.dirname(filePath);
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
  fs.writeFileSync(filePath, JSON.stringify(data, null, 2) + '\n', 'utf-8');
}

// Hook/rule templates carry the product name, so injected entries are
// detected by substring. Current installs contain "astria"; pre-1.0
// installs contain "graphify" (never both) — inject upgrades the legacy
// entries in place and uninstall removes either era.
const isCurrent = (s: string): boolean => s.includes('astria');
const isLegacy = (s: string): boolean => !s.includes('astria') && s.includes('graphify');
const isAnyEra = (s: string): boolean => s.includes('astria') || s.includes('graphify');

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
  // OpenCode 1.17+ auto-discovers plugins from `.opencode/plugin/` and
  // REJECTS a `plugins` key in opencode.json ("Unrecognized key"), so the
  // plugin is a file drop with no config registration. The pre-1.0.9
  // `plugins/` directory and config key are cleaned up on inject.
  const pluginDir = path.join(projectDir, '.opencode', 'plugin');
  const pluginPath = path.join(pluginDir, 'astria.js');
  const legacyDir = path.join(projectDir, '.opencode', 'plugins');
  const legacyPaths = [path.join(legacyDir, 'graphify.js'), path.join(legacyDir, 'astria.js')];

  let changed = false;
  for (const legacy of legacyPaths) {
    if (fs.existsSync(legacy)) {
      fs.unlinkSync(legacy);
      changed = true;
    }
  }
  // Strip the now-invalid `plugins` key this installer used to write.
  const configPath = path.join(projectDir, '.opencode', 'opencode.json');
  const config = readJson(configPath);
  if (Array.isArray(config.plugins) && config.plugins.length > 0) {
    delete config.plugins;
    writeJson(configPath, config);
    changed = true;
  }

  if (fs.existsSync(pluginPath)) return changed;
  if (!fs.existsSync(pluginDir)) {
    fs.mkdirSync(pluginDir, { recursive: true });
  }
  fs.writeFileSync(pluginPath, OPENCODE_PLUGIN_JS, 'utf-8');
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
  const configPath = path.join(projectDir, '.opencode', 'opencode.json');
  const config = readJson(configPath);
  if (Array.isArray(config.plugins)) {
    delete config.plugins;
    writeJson(configPath, config);
    changed = true;
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
  fs.writeFileSync(rulePath, CURSOR_RULE, 'utf-8');
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
  | 'copilot'
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
  // Kiro workspace MCP servers live in a bare mcp.json at the project root.
  kiro: { configPath: 'mcp.json', serverPath: ['mcpServers', 'astria'] },
  // OpenCode keys servers directly under `mcp` (no `servers` intermediate)
  // and requires its own server shape. The 1.0.8-era nested path is
  // migrated away on inject.
  opencode: {
    configPath: path.join('.opencode', 'opencode.json'),
    serverPath: ['mcp', 'astria'],
    server: { type: 'local', command: ['astria', 'mcp'], enabled: true },
    legacyServerPath: ['mcp', 'servers', 'astria'],
  },
  // GitHub Copilot coding agent (repo-level pre-configuration).
  copilot: { configPath: path.join('.github', 'copilot-mcp.json'), serverPath: ['servers', 'astria'] },
};
const LEGACY_MCP_SERVER_NAME = 'graphify';

// ---- Codex MCP (~/.codex/config.toml, user-global TOML) ----

// The managed table is deterministic so removal can verify it is ours and
// never touch a hand-written [mcp_servers.astria] section.
const CODEX_MCP_BLOCK = '[mcp_servers.astria]\ncommand = "astria"\nargs = ["mcp"]';

function codexConfigPath(): string {
  return path.join(os.homedir(), '.codex', 'config.toml');
}

export function injectCodexMcp(): boolean {
  const configPath = codexConfigPath();
  let text = '';
  try {
    text = fs.readFileSync(configPath, 'utf8');
  } catch {
    // absent — created below
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
  fs.mkdirSync(path.dirname(configPath), { recursive: true });
  fs.writeFileSync(configPath, next, 'utf8');
  return true;
}

export function removeCodexMcp(): boolean {
  const configPath = codexConfigPath();
  let text: string;
  try {
    text = fs.readFileSync(configPath, 'utf8');
  } catch {
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
  fs.writeFileSync(configPath, next, 'utf8');
  return true;
}

export function injectAgentMcp(projectDir: string, flavor: McpFlavor): boolean {
  if (flavor === 'codex') {
    return injectCodexMcp();
  }
  const target = MCP_TARGETS[flavor]!;
  const configPath = path.join(projectDir, target.configPath);
  const data = readJson(configPath);

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
    return false;
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
    return removeCodexMcp();
  }
  const target = MCP_TARGETS[flavor]!;
  const configPath = path.join(projectDir, target.configPath);
  if (!fs.existsSync(configPath)) return false;

  const data = readJson(configPath);
  const parents: Array<[any, string]> = [];
  let node: any = data;
  for (const key of target.serverPath.slice(0, -1)) {
    if (typeof node[key] !== 'object' || node[key] === null) return false;
    parents.push([node, key]);
    node = node[key];
  }
  const name = target.serverPath[target.serverPath.length - 1];

  // The legacy-name and since-moved-path entries go too when present.
  let removed = Boolean(node[name]) || Boolean(node[LEGACY_MCP_SERVER_NAME]);
  delete node[name];
  delete node[LEGACY_MCP_SERVER_NAME];
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
      if (legacyNode[legacyName] || legacyNode[LEGACY_MCP_SERVER_NAME]) removed = true;
      delete legacyNode[legacyName];
      delete legacyNode[LEGACY_MCP_SERVER_NAME];
      for (let i = legacyParents.length - 1; i >= 0; i--) {
        const [parent, key] = legacyParents[i];
        if (Object.keys(parent[key]).length === 0) delete parent[key];
      }
    }
  }
  if (!removed) return false;
  for (let i = parents.length - 1; i >= 0; i--) {
    const [parent, key] = parents[i];
    if (Object.keys(parent[key]).length === 0) delete parent[key];
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
  fs.writeFileSync(steerPath, KIRO_STEERING, 'utf-8');
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
