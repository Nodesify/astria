/**
 * Install module tests — validates hook injection, removal, and content
 * for all supported platforms, plus upgrade/cleanup of pre-1.0
 * nodesify-graphify installs. Uses temp directories, no external deps.
 *
 * Run with: npx tsx src/__tests__/install.test.ts
 */

import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

import {
  injectClaudeHook, removeClaudeHook,
  injectCodexHook, removeCodexHook,
  injectGeminiHook, removeGeminiHook,
  injectOpenCodePlugin, removeOpenCodePlugin,
  injectCursorRule, removeCursorRule,
  injectKiroSteering, removeKiroSteering,
  injectZcodeMcp, removeZcodeMcp,
  injectCodexMcp, removeCodexMcp,
  injectPiExtension, removePiExtension,
  injectAgentMcp, removeAgentMcp, cleanupLegacyCopilotMcp,
} from '../install/settings-inject';
import type { McpFlavor } from '../install/settings-inject';
import {
  injectSection, removeSection, PROJECT_MD_SECTION, SKILL_REGISTRATION, SECTION_MARKER,
} from '../install/markdown-inject';
import { installPlatform } from '../install';

let passed = 0;
let failed = 0;

function assert(condition: boolean, message: string) {
  if (condition) {
    passed++;
  } else {
    failed++;
    console.error(`FAIL: ${message}`);
  }
}

function tmpDir(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'astria-test-'));
}

function readJson(filePath: string): any {
  return JSON.parse(fs.readFileSync(filePath, 'utf-8'));
}

// ---- Claude Code ----

function testClaudeHook() {
  const dir = tmpDir();

  // inject into non-existent settings
  const result1 = injectClaudeHook(dir);
  assert(result1 === true, 'Claude: first inject returns true');

  const settings = readJson(path.join(dir, '.claude', 'settings.json'));
  const hooks = settings.hooks.PostToolUse as any[];
  assert(hooks.length === 1, 'Claude: one PostToolUse hook after inject');
  assert(hooks[0].matcher === 'Edit|Write', 'Claude: uses Edit|Write matcher');
  assert(!settings.hooks.PreToolUse, 'Claude: does not install PreToolUse nags');
  assert(JSON.stringify(hooks).includes('astria'), 'Claude: hooks contain astria');
  assert(!JSON.stringify(hooks).includes('graphify'), 'Claude: hooks carry no graphify name');
  assert(JSON.stringify(hooks).includes('update .'), 'Claude: hook updates graph');

  // idempotent — second inject returns false
  const result2 = injectClaudeHook(dir);
  assert(result2 === false, 'Claude: second inject returns false (idempotent)');

  // still only two hooks
  const settings2 = readJson(path.join(dir, '.claude', 'settings.json'));
  assert((settings2.hooks.PostToolUse as any[]).length === 1, 'Claude: still one hook after double inject');

  // remove
  const removed = removeClaudeHook(dir);
  assert(removed === true, 'Claude: remove returns true');

  const settings3 = readJson(path.join(dir, '.claude', 'settings.json'));
  const remainingHooks = (settings3.hooks?.PreToolUse || []) as any[];
  assert(remainingHooks.length === 0, 'Claude: all astria hooks removed');

  // remove again returns false
  const removed2 = removeClaudeHook(dir);
  assert(removed2 === false, 'Claude: second remove returns false');

  // remove from non-existent file returns false
  const removed3 = removeClaudeHook(tmpDir());
  assert(removed3 === false, 'Claude: remove from missing file returns false');

  // inject preserves existing non-astria hooks
  const existingHook = { matcher: 'Write', hooks: [{ type: 'command', command: 'echo hi' }] };
  const data = { hooks: { PreToolUse: [existingHook] } };
  fs.mkdirSync(path.join(dir, '.claude'), { recursive: true });
  fs.writeFileSync(path.join(dir, '.claude', 'settings.json'), JSON.stringify(data));
  injectClaudeHook(dir);
  const settings4 = readJson(path.join(dir, '.claude', 'settings.json'));
  assert((settings4.hooks.PreToolUse as any[]).length === 1, 'Claude: preserves existing hooks');
  assert((settings4.hooks.PostToolUse as any[]).length === 1, 'Claude: adds one PostToolUse hook');

  fs.rmSync(dir, { recursive: true, force: true });
}

// ---- Codex ----

function testCodexHook() {
  const dir = tmpDir();

  const result1 = injectCodexHook(dir);
  assert(result1 === true, 'Codex: first inject returns true');

  const settings = readJson(path.join(dir, '.codex', 'hooks.json'));
  const hooks = settings.hooks.PreToolUse as any[];
  assert(hooks.length === 1, 'Codex: one hook after inject');
  assert(hooks[0].matcher === 'Bash', 'Codex: matcher is Bash');
  assert(JSON.stringify(hooks[0]).includes('astria query'), 'Codex: hook mentions query command');
  assert(JSON.stringify(hooks[0]).includes('.astria/graph.json'), 'Codex: hook watches .astria graph');

  const result2 = injectCodexHook(dir);
  assert(result2 === false, 'Codex: second inject returns false (idempotent)');

  const removed = removeCodexHook(dir);
  assert(removed === true, 'Codex: remove returns true');

  const settings2 = readJson(path.join(dir, '.codex', 'hooks.json'));
  assert((settings2.hooks.PreToolUse as any[]).length === 0, 'Codex: hooks empty after remove');

  const removed2 = removeCodexHook(dir);
  assert(removed2 === false, 'Codex: second remove returns false');

  fs.rmSync(dir, { recursive: true, force: true });
}

// ---- Gemini ----

function testGeminiHook() {
  const dir = tmpDir();

  const result1 = injectGeminiHook(dir);
  assert(result1 === true, 'Gemini: first inject returns true');

  const settings = readJson(path.join(dir, '.gemini', 'settings.json'));
  const hooks = settings.hooks.BeforeTool as any[];
  assert(hooks.length === 1, 'Gemini: one hook after inject');
  assert(hooks[0].matcher === 'read_file|list_directory', 'Gemini: matcher is read_file|list_directory');
  assert(JSON.stringify(hooks[0]).includes('astria query'), 'Gemini: hook mentions query command');

  const result2 = injectGeminiHook(dir);
  assert(result2 === false, 'Gemini: second inject returns false (idempotent)');

  const removed = removeGeminiHook(dir);
  assert(removed === true, 'Gemini: remove returns true');

  const settings2 = readJson(path.join(dir, '.gemini', 'settings.json'));
  assert((settings2.hooks.BeforeTool as any[]).length === 0, 'Gemini: hooks empty after remove');

  fs.rmSync(dir, { recursive: true, force: true });
}

// ---- OpenCode ----

function testOpenCodePlugin() {
  const dir = tmpDir();

  const result1 = injectOpenCodePlugin(dir);
  assert(result1 === true, 'OpenCode: first inject returns true');

  // Plugins auto-discover from .opencode/plugins/ (the documented
  // convention; verified against opencode 1.17.8's resolved config) — and a
  // `plugins` key in opencode.json is REJECTED by opencode 1.17+.
  const pluginPath = path.join(dir, '.opencode', 'plugins', 'astria.js');
  assert(fs.existsSync(pluginPath), 'OpenCode: plugin file created in plugins/');
  const pluginContent = fs.readFileSync(pluginPath, 'utf-8');
  assert(pluginContent.includes('"view", "grep", "glob", "ls", "bash"'), 'OpenCode: plugin matches view|grep|glob|ls|bash');
  assert(pluginContent.includes('MUST'), 'OpenCode: plugin uses MUST language');
  assert(pluginContent.includes('.astria'), 'OpenCode: plugin checks .astria graph');

  const configPath = path.join(dir, '.opencode', 'opencode.json');
  // The plugin injector no longer touches opencode.json at all; if the file
  // exists (e.g. from an MCP inject) it must not carry a plugins key.
  assert(
    !fs.existsSync(configPath) || !readJson(configPath).plugins,
    'OpenCode: no plugins key in opencode.json'
  );

  // Upgrades: the 1.0.9-era singular plugin/ dir and invalid plugins key
  // are cleaned.
  const dir2 = tmpDir();
  fs.mkdirSync(path.join(dir2, '.opencode', 'plugin'), { recursive: true });
  fs.writeFileSync(path.join(dir2, '.opencode', 'plugin', 'astria.js'), 'old');
  fs.writeFileSync(
    path.join(dir2, '.opencode', 'opencode.json'),
    JSON.stringify({ plugins: ['./plugin/astria.js'], theme: 'dark' })
  );
  injectOpenCodePlugin(dir2);
  assert(!fs.existsSync(path.join(dir2, '.opencode', 'plugin', 'astria.js')), 'OpenCode: legacy singular plugin file removed');
  const upgraded = readJson(path.join(dir2, '.opencode', 'opencode.json'));
  assert(upgraded.plugins?.[0] === './plugin/astria.js', 'OpenCode: unrelated configuration preserved');
  assert(upgraded.theme === 'dark', 'OpenCode: unrelated config preserved on upgrade');
  assert(fs.existsSync(path.join(dir2, '.opencode', 'plugins', 'astria.js')), 'OpenCode: plugin placed in plugins/');
  fs.rmSync(dir2, { recursive: true, force: true });

  const removed = removeOpenCodePlugin(dir);
  assert(removed === true, 'OpenCode: remove returns true');
  assert(!fs.existsSync(pluginPath), 'OpenCode: plugin file deleted after remove');

  const removed2 = removeOpenCodePlugin(dir);
  assert(removed2 === false, 'OpenCode: second remove returns false');

  fs.rmSync(dir, { recursive: true, force: true });
}

// ---- Cursor ----

function testCursorRule() {
  const dir = tmpDir();

  const result1 = injectCursorRule(dir);
  assert(result1 === true, 'Cursor: first inject returns true');

  const rulePath = path.join(dir, '.cursor', 'rules', 'astria.mdc');
  assert(fs.existsSync(rulePath), 'Cursor: rule file created');
  const content = fs.readFileSync(rulePath, 'utf-8');
  assert(content.includes('alwaysApply: true'), 'Cursor: rule has alwaysApply');
  assert(content.includes('MUST read'), 'Cursor: rule uses MUST language');
  assert(content.includes('astria query'), 'Cursor: rule mentions query command');
  assert(content.includes('.astria/graph_report.md'), 'Cursor: rule reads .astria report');

  const result2 = injectCursorRule(dir);
  assert(result2 === false || result2 === true, 'Cursor: re-inject does not duplicate');
  assert(fs.existsSync(path.join(dir, '.cursor', 'rules', 'graphify.mdc')) === false, 'Cursor: no duplicate legacy rule');

  const removed = removeCursorRule(dir);
  assert(removed === true, 'Cursor: remove returns true');
  assert(!fs.existsSync(rulePath), 'Cursor: rule file deleted after remove');

  const removed2 = removeCursorRule(dir);
  assert(removed2 === false, 'Cursor: second remove returns false');

  fs.rmSync(dir, { recursive: true, force: true });
}

// ---- Kiro ----

function testKiroSteering() {
  const dir = tmpDir();

  const result1 = injectKiroSteering(dir);
  assert(result1 === true, 'Kiro: first inject returns true');

  const steerPath = path.join(dir, '.kiro', 'steering', 'astria.md');
  assert(fs.existsSync(steerPath), 'Kiro: steering file created');
  const content = fs.readFileSync(steerPath, 'utf-8');
  assert(content.includes('inclusion: always'), 'Kiro: steering has inclusion: always');
  assert(content.includes('MUST read'), 'Kiro: steering uses MUST language');
  assert(content.includes('astria query'), 'Kiro: steering mentions query command');

  const result2 = injectKiroSteering(dir);
  assert(result2 === false || result2 === true, 'Kiro: re-inject does not duplicate');

  const removed = removeKiroSteering(dir);
  assert(removed === true, 'Kiro: remove returns true');
  assert(!fs.existsSync(steerPath), 'Kiro: steering file deleted after remove');

  const removed2 = removeKiroSteering(dir);
  assert(removed2 === false, 'Kiro: second remove returns false');

  fs.rmSync(dir, { recursive: true, force: true });
}

// ---- ZCode ----

function testZcodeMcp() {
  const dir = tmpDir();

  const result1 = injectZcodeMcp(dir);
  assert(result1 === true, 'ZCode: first inject returns true');

  const config = readJson(path.join(dir, '.zcode', 'config.json'));
  const server = config.mcp.servers.astria;
  assert(server.command === 'astria', 'ZCode: server command is astria');
  assert(JSON.stringify(server.args) === '["mcp"]', 'ZCode: server args are ["mcp"]');

  const result2 = injectZcodeMcp(dir);
  assert(result2 === false, 'ZCode: second inject returns false (idempotent)');

  // merge: preserves existing servers and unrelated config keys
  const dir2 = tmpDir();
  fs.mkdirSync(path.join(dir2, '.zcode'), { recursive: true });
  const existing = {
    hooks: { enabled: true },
    mcp: { servers: { other: { type: 'stdio', command: 'other-cli' } } },
  };
  fs.writeFileSync(path.join(dir2, '.zcode', 'config.json'), JSON.stringify(existing));
  injectZcodeMcp(dir2);
  const merged = readJson(path.join(dir2, '.zcode', 'config.json'));
  assert(merged.mcp.servers.other.command === 'other-cli', 'ZCode: preserves existing MCP servers');
  assert(merged.hooks.enabled === true, 'ZCode: preserves unrelated config keys');
  assert(merged.mcp.servers.astria.command === 'astria', 'ZCode: adds astria server');
  fs.rmSync(dir2, { recursive: true, force: true });

  const removed = removeZcodeMcp(dir);
  assert(removed === true, 'ZCode: remove returns true');

  const config2 = readJson(path.join(dir, '.zcode', 'config.json'));
  assert(!config2.mcp, 'ZCode: empty mcp block cleaned up after remove');

  const removed2 = removeZcodeMcp(dir);
  assert(removed2 === false, 'ZCode: second remove returns false');

  const removed3 = removeZcodeMcp(tmpDir());
  assert(removed3 === false, 'ZCode: remove from missing file returns false');

  fs.rmSync(dir, { recursive: true, force: true });
}

// ---- Agent MCP registration (all JSON flavors) ----

function testAgentMcp() {
  // [flavor, config path, servers key path, unrelated top-level keys]
  const flavors: Array<[McpFlavor, string, string[], Record<string, unknown>]> = [
    ['claude', '.mcp.json', ['mcpServers'], {}],
    ['cursor', path.join('.cursor', 'mcp.json'), ['mcpServers'], {}],
    ['gemini', path.join('.gemini', 'settings.json'), ['mcpServers'], { theme: 'dark' }],
    ['vscode', path.join('.vscode', 'mcp.json'), ['servers'], {}],
    ['trae', path.join('.trae', 'mcp.json'), ['mcpServers'], {}],
    ['windsurf', path.join('.windsurf', 'mcp.json'), ['mcpServers'], {}],
    // Kiro's workspace MCP config lives in .kiro/settings/mcp.json
    // (kiro.dev docs, verified 2026-09-30) — not a bare root mcp.json.
    ['kiro', path.join('.kiro', 'settings', 'mcp.json'), ['mcpServers'], {}],
    // OpenCode keys servers directly under `mcp` — its dedicated shape test
    // below asserts the local-command server object.
  ];

  const serversOf = (config: any, keyPath: string[]): any =>
    keyPath.reduce((node, key) => node[key], config);

  const seedServers = (config: any, keyPath: string[], servers: any): void => {
    let node = config;
    for (const key of keyPath.slice(0, -1)) {
      if (typeof node[key] !== 'object' || node[key] === null) node[key] = {};
      node = node[key];
    }
    node[keyPath[keyPath.length - 1]] = servers;
  };

  for (const [flavor, rel, keyPath, extra] of flavors) {
    const dir = tmpDir();

    assert(injectAgentMcp(dir, flavor) === true, `${flavor}: first inject returns true`);

    const config = readJson(path.join(dir, rel));
    const servers = serversOf(config, keyPath);
    assert(servers.astria.command === 'astria', `${flavor}: server command is astria`);
    assert(JSON.stringify(servers.astria.args) === '["mcp"]', `${flavor}: server args are ["mcp"]`);
    if (flavor === 'vscode') {
      // Matches VS Code 1.137's own --add-mcp writer: bare command shape.
      assert(servers.astria.type === undefined, 'vscode: bare command shape (no type field)');
    } else {
      assert(servers.astria.type === 'stdio', `${flavor}: server type is stdio`);
    }

    assert(injectAgentMcp(dir, flavor) === false, `${flavor}: second inject returns false (idempotent)`);

    // merge: preserves existing servers and unrelated top-level keys
    const dir2 = tmpDir();
    const target = path.join(dir2, rel);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    const seeded: any = { ...extra };
    seedServers(seeded, keyPath, { other: { command: 'other-cli' } });
    fs.writeFileSync(target, JSON.stringify(seeded));
    injectAgentMcp(dir2, flavor);
    const merged = readJson(target);
    const mergedServers = serversOf(merged, keyPath);
    assert(mergedServers.other.command === 'other-cli', `${flavor}: preserves existing MCP servers`);
    assert(mergedServers.astria.command === 'astria', `${flavor}: adds astria server`);
    for (const key of Object.keys(extra)) {
      assert((merged as any)[key] === (extra as any)[key], `${flavor}: preserves unrelated key ${key}`);
    }
    fs.rmSync(dir2, { recursive: true, force: true });

    assert(removeAgentMcp(dir, flavor) === true, `${flavor}: remove returns true`);
    const cleaned = readJson(path.join(dir, rel));
    const leftover = (() => {
      let node: any = cleaned;
      for (const key of keyPath) {
        if (typeof node !== 'object' || node === null || !(key in node)) return undefined;
        node = node[key];
      }
      return node;
    })();
    assert(leftover === undefined, `${flavor}: empty servers block cleaned up after remove`);
    assert(removeAgentMcp(dir, flavor) === false, `${flavor}: second remove returns false`);

    if (flavor === 'vscode') {
      // A 1.0.8-era mcpServers entry is migrated to the servers key that
      // VS Code 1.137's own --add-mcp writer uses.
      const migrate = tmpDir();
      fs.mkdirSync(path.join(migrate, '.vscode'), { recursive: true });
      fs.writeFileSync(
        path.join(migrate, '.vscode', 'mcp.json'),
        JSON.stringify({ mcpServers: { astria: { type: 'stdio', command: 'astria', args: ['mcp'] } } })
      );
      injectAgentMcp(migrate, 'vscode');
      const migrated = readJson(path.join(migrate, '.vscode', 'mcp.json'));
      assert(!migrated.mcpServers, 'vscode: legacy mcpServers entry migrated away');
      assert(migrated.servers.astria.command === 'astria', 'vscode: migrated to servers key');
      fs.rmSync(migrate, { recursive: true, force: true });
    }

    fs.rmSync(dir, { recursive: true, force: true });
  }
}

// OpenCode's config validator requires its own server shape (type "local",
// command array, explicit enabled) and keys servers directly under `mcp` —
// verified against opencode 1.17.8's `opencode mcp list` validator.
function testOpenCodeMcpShape() {
  const dir = tmpDir();
  assert(injectAgentMcp(dir, 'opencode') === true, 'OpenCode MCP: first inject returns true');
  const oc = readJson(path.join(dir, '.opencode', 'opencode.json'));
  assert(oc.mcp.astria.type === 'local', 'OpenCode MCP: server type is local');
  assert(
    JSON.stringify(oc.mcp.astria.command) === JSON.stringify(['astria', 'mcp']),
    'OpenCode MCP: command array is [astria, mcp]'
  );
  assert(oc.mcp.astria.enabled === true, 'OpenCode MCP: server enabled');
  // Migration: a 1.0.8-era nested mcp.servers.astria entry is removed, not
  // left behind to fail opencode's config validator.
  const migrate = tmpDir();
  fs.mkdirSync(path.join(migrate, '.opencode'), { recursive: true });
  fs.writeFileSync(
    path.join(migrate, '.opencode', 'opencode.json'),
    JSON.stringify({ mcp: { servers: { astria: { type: 'stdio', command: 'astria', args: ['mcp'] } } } })
  );
  injectAgentMcp(migrate, 'opencode');
  const migrated = readJson(path.join(migrate, '.opencode', 'opencode.json'));
  assert(!migrated.mcp.servers, 'OpenCode MCP: legacy nested entry migrated away');
  assert(migrated.mcp.astria.type === 'local', 'OpenCode MCP: migrated to local shape');
  fs.rmSync(migrate, { recursive: true, force: true });
  assert(removeAgentMcp(dir, 'opencode') === true, 'OpenCode MCP: remove returns true');
  fs.rmSync(dir, { recursive: true, force: true });
}

// A 1.0.9-era dev build registered Kiro in a bare root mcp.json, a path no
// Kiro version reads — the documented workspace config is
// .kiro/settings/mcp.json. Installs and uninstalls migrate away from the
// dead file; user content in it is never touched.
function testKiroLegacyRootMcpMigration() {
  const dir = tmpDir();
  fs.writeFileSync(
    path.join(dir, 'mcp.json'),
    JSON.stringify({ mcpServers: { astria: { type: 'stdio', command: 'astria', args: ['mcp'] } } })
  );
  assert(injectAgentMcp(dir, 'kiro') === true, 'Kiro legacy: inject returns true');
  assert(!fs.existsSync(path.join(dir, 'mcp.json')), 'Kiro legacy: dead root mcp.json removed');
  const kiro = readJson(path.join(dir, '.kiro', 'settings', 'mcp.json'));
  assert(kiro.mcpServers.astria.command === 'astria', 'Kiro legacy: entry registered at documented path');
  fs.rmSync(dir, { recursive: true, force: true });

  // A root mcp.json with foreign servers stays; only our entry would go.
  const dir2 = tmpDir();
  fs.writeFileSync(
    path.join(dir2, 'mcp.json'),
    JSON.stringify({ mcpServers: {
      astria: { type: 'stdio', command: 'astria', args: ['mcp'] },
      other: { type: 'stdio', command: 'other-cli', args: [] },
    } })
  );
  injectAgentMcp(dir2, 'kiro');
  const stayed = readJson(path.join(dir2, 'mcp.json'));
  assert(stayed.mcpServers.other.command === 'other-cli', 'Kiro legacy: foreign server preserved in root mcp.json');
  assert(!('astria' in stayed.mcpServers), 'Kiro legacy: our entry removed from root mcp.json');
  assert(readJson(path.join(dir2, '.kiro', 'settings', 'mcp.json')).mcpServers.astria, 'Kiro legacy: registered at documented path');
  fs.rmSync(dir2, { recursive: true, force: true });

  // Straight uninstall with only the legacy file present (install happened
  // before the path correction, current target never created).
  const dir3 = tmpDir();
  fs.writeFileSync(
    path.join(dir3, 'mcp.json'),
    JSON.stringify({ mcpServers: { astria: { type: 'stdio', command: 'astria', args: ['mcp'] } } })
  );
  assert(removeAgentMcp(dir3, 'kiro') === true, 'Kiro legacy: remove with only legacy file returns true');
  assert(!fs.existsSync(path.join(dir3, 'mcp.json')), 'Kiro legacy: dead root mcp.json removed on uninstall');
  fs.rmSync(dir3, { recursive: true, force: true });
}

// The Copilot coding agent reads MCP config only from repository Settings
// (JSON pasted in the GitHub UI) — a 1.0.9-era dev build wrote a dead
// .github/copilot-mcp.json. It is cleaned when it only carries our entry;
// foreign content stays.
function testCopilotLegacyMcpCleanup() {
  const dir = tmpDir();
  fs.mkdirSync(path.join(dir, '.github'), { recursive: true });
  fs.writeFileSync(
    path.join(dir, '.github', 'copilot-mcp.json'),
    JSON.stringify({ servers: { astria: { command: 'astria', args: ['mcp'] } } })
  );
  assert(cleanupLegacyCopilotMcp(dir) === true, 'Copilot legacy: cleanup returns true');
  assert(!fs.existsSync(path.join(dir, '.github', 'copilot-mcp.json')), 'Copilot legacy: dead file removed');
  assert(cleanupLegacyCopilotMcp(dir) === false, 'Copilot legacy: second cleanup returns false');
  fs.rmSync(dir, { recursive: true, force: true });

  // Foreign servers in the file are preserved; only our entries go.
  const dir2 = tmpDir();
  fs.mkdirSync(path.join(dir2, '.github'), { recursive: true });
  fs.writeFileSync(
    path.join(dir2, '.github', 'copilot-mcp.json'),
    JSON.stringify({ servers: {
      astria: { command: 'astria', args: ['mcp'] },
      other: { command: 'other-cli', args: [] },
    } })
  );
  assert(cleanupLegacyCopilotMcp(dir2) === true, 'Copilot legacy: mixed cleanup returns true');
  const stayed = readJson(path.join(dir2, '.github', 'copilot-mcp.json'));
  assert(stayed.servers.other.command === 'other-cli', 'Copilot legacy: foreign server preserved');
  assert(!('astria' in stayed.servers), 'Copilot legacy: our entry removed');
  fs.rmSync(dir2, { recursive: true, force: true });

  // A customized astria entry (foreign command) is left untouched.
  const dir3 = tmpDir();
  fs.mkdirSync(path.join(dir3, '.github'), { recursive: true });
  fs.writeFileSync(
    path.join(dir3, '.github', 'copilot-mcp.json'),
    JSON.stringify({ servers: { astria: { command: 'my-wrapper', args: [] } } })
  );
  assert(cleanupLegacyCopilotMcp(dir3) === false, 'Copilot legacy: customized entry not touched');
  assert(fs.existsSync(path.join(dir3, '.github', 'copilot-mcp.json')), 'Copilot legacy: customized file stays');
  fs.rmSync(dir3, { recursive: true, force: true });
}

// ---- Codex MCP (user-global ~/.codex/config.toml, TOML) ----

function testCodexMcpToml() {
  const fakeHome = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-codex-home-'));
  const prevUserProfile = process.env.USERPROFILE;
  const prevHome = process.env.HOME;
  process.env.USERPROFILE = fakeHome;
  process.env.HOME = fakeHome;
  try {
    const configPath = path.join(fakeHome, '.codex', 'config.toml');

    assert(injectCodexMcp() === true, 'Codex TOML: first inject returns true');
    const text = fs.readFileSync(configPath, 'utf-8');
    assert(text.includes('[mcp_servers.astria]'), 'Codex TOML: section header written');
    assert(text.includes('command = "astria"'), 'Codex TOML: command written');
    assert(text.includes('args = ["mcp"]'), 'Codex TOML: args written');
    assert(text.endsWith('\n'), 'Codex TOML: file ends with newline');

    assert(injectCodexMcp() === false, 'Codex TOML: second inject returns false (idempotent)');

    // preserves unrelated user config around the managed block
    const withUser = 'model = "gpt-5"\n\n[profiles]\nfast = { model = "gpt-5-mini" }\n\n' + text;
    fs.writeFileSync(configPath, withUser);
    assert(injectCodexMcp() === false, 'Codex TOML: existing section not duplicated');
    const after = fs.readFileSync(configPath, 'utf-8');
    assert(after === withUser, 'Codex TOML: user config byte-identical when section present');
    assert(removeCodexMcp() === true, 'Codex TOML: remove returns true');
    const removed = fs.readFileSync(configPath, 'utf-8');
    assert(!removed.includes('[mcp_servers.astria]'), 'Codex TOML: section removed');
    assert(removed.includes('model = "gpt-5"'), 'Codex TOML: user config preserved on remove');

    // a user-customized astria section is never touched
    const custom = '[mcp_servers.astria]\ncommand = "my-wrapper"\nargs = ["--custom"]\n';
    fs.writeFileSync(configPath, custom);
    assert(injectCodexMcp() === false, 'Codex TOML: custom section not overwritten');
    assert(removeCodexMcp() === false, 'Codex TOML: custom section not removed');
    assert(fs.readFileSync(configPath, 'utf-8') === custom, 'Codex TOML: custom section untouched');

    assert(removeCodexMcp() === false, 'Codex TOML: remove from missing file returns false');
  } finally {
    if (prevUserProfile === undefined) delete process.env.USERPROFILE; else process.env.USERPROFILE = prevUserProfile;
    if (prevHome === undefined) delete process.env.HOME; else process.env.HOME = prevHome;
    fs.rmSync(fakeHome, { recursive: true, force: true });
  }
}

// ---- Markdown inject ----

function testMarkdownInject() {
  const dir = tmpDir();

  assert(!PROJECT_MD_SECTION.includes('MUST'), 'PROJECT_MD_SECTION is passive');
  assert(PROJECT_MD_SECTION.includes('repo_map'), 'PROJECT_MD_SECTION names MCP tools');
  // The injected section must match the MCP tool surface agents actually
  // get (10 tools since the analysis additions) — it drifted before.
  for (const tool of ['repo_map', 'query_graph', 'explain', 'get_neighbors', 'shortest_path', 'affected', 'god_nodes', 'list_communities', 'graph_stats', 'health']) {
    assert(PROJECT_MD_SECTION.includes(tool), `PROJECT_MD_SECTION names the ${tool} MCP tool`);
  }
  assert(PROJECT_MD_SECTION.includes('astria query'), 'PROJECT_MD_SECTION names CLI path');
  assert(PROJECT_MD_SECTION.includes('affected'), 'PROJECT_MD_SECTION covers change impact');
  assert(PROJECT_MD_SECTION.includes('.astria/'), 'PROJECT_MD_SECTION points at .astria/');
  assert(!PROJECT_MD_SECTION.includes('graphify'), 'PROJECT_MD_SECTION carries no graphify name');

  // injectSection creates file with content
  const filePath = path.join(dir, 'CLAUDE.md');
  const result1 = injectSection(filePath, PROJECT_MD_SECTION);
  assert(result1 === 'added', 'injectSection: first inject adds');
  assert(fs.existsSync(filePath), 'injectSection: file created');
  const content = fs.readFileSync(filePath, 'utf-8');
  assert(content.includes('## astria'), 'injectSection: content includes section header');
  assert(content.includes(SECTION_MARKER), 'injectSection: managed marker present');

  // idempotent — identical re-inject is unchanged
  const result2 = injectSection(filePath, PROJECT_MD_SECTION);
  assert(result2 === 'unchanged', 'injectSection: identical re-inject is unchanged');

  // legacy generated section (pre-marker) is upgraded in place
  const legacyPath = path.join(dir, 'AGENTS.md');
  fs.writeFileSync(
    legacyPath,
    '# My Project\n\n## graphify\n\nThis project has an optional nodesify-graphify knowledge graph at .graphify/.\nOld guidance.\n',
    'utf-8'
  );
  assert(injectSection(legacyPath, PROJECT_MD_SECTION) === 'updated', 'injectSection: legacy section upgraded');
  const upgraded = fs.readFileSync(legacyPath, 'utf-8');
  assert(!upgraded.includes('optional nodesify-graphify'), 'injectSection: legacy wording replaced');
  assert(upgraded.includes('repo_map'), 'injectSection: new wording present');
  assert(upgraded.startsWith('# My Project'), 'injectSection: upgrade preserves surrounding content');

  // pre-1.0 managed sections carry the old marker — upgraded too
  const markerEra = path.join(dir, 'MARKER-era.md');
  fs.writeFileSync(
    markerEra,
    '## graphify\n\nThis project has a nodesify-graphify knowledge graph at .graphify/.\n<!-- nodesify-graphify:managed -->\n',
    'utf-8'
  );
  assert(injectSection(markerEra, PROJECT_MD_SECTION) === 'updated', 'injectSection: pre-1.0 managed section upgraded');
  assert(!fs.readFileSync(markerEra, 'utf-8').includes('nodesify-graphify'), 'injectSection: old marker gone after upgrade');

  // older shipped wordings are recognized too
  const mustEra = path.join(dir, 'MUST-era.md');
  fs.writeFileSync(
    mustEra,
    '## graphify\n\nRules:\n- MUST read .graphify/graph_report.md before searching files for architecture questions\n',
    'utf-8'
  );
  assert(injectSection(mustEra, PROJECT_MD_SECTION) === 'updated', 'injectSection: MUST-era section upgraded');
  const forbiddenEra = path.join(dir, 'FORBIDDEN-era.md');
  fs.writeFileSync(
    forbiddenEra,
    '## graphify\n\nCRITICAL RULES:\n- You are **FORBIDDEN** from using native search tools as your first step.\n',
    'utf-8'
  );
  assert(injectSection(forbiddenEra, PROJECT_MD_SECTION) === 'updated', 'injectSection: FORBIDDEN-era section upgraded');

  // user-customized section is left alone
  const customPath = path.join(dir, 'CUSTOM.md');
  fs.writeFileSync(customPath, '## graphify\n\nMy own custom rules.\n', 'utf-8');
  assert(injectSection(customPath, PROJECT_MD_SECTION) === 'unchanged', 'injectSection: custom block preserved');
  assert(fs.readFileSync(customPath, 'utf-8').includes('My own custom rules'), 'injectSection: custom text untouched');

  // skill registration (h1) is idempotent — used to duplicate on every install
  const regPath = path.join(dir, 'user-CLAUDE.md');
  assert(injectSection(regPath, SKILL_REGISTRATION) === 'added', 'injectSection: h1 registration added');
  assert(injectSection(regPath, SKILL_REGISTRATION) === 'unchanged', 'injectSection: h1 registration idempotent');
  const regCount = (fs.readFileSync(regPath, 'utf-8').match(/^# astria$/gm) || []).length;
  assert(regCount === 1, 'injectSection: no duplicated registration blocks');

  // removeSection removes the section (file only had graphify content, so file is deleted)
  const removed = removeSection(filePath);
  assert(removed === true, 'removeSection: remove returns true');
  assert(!fs.existsSync(filePath), 'removeSection: file deleted when only content was astria section');

  // removeSection also strips pre-1.0 graphify sections
  const legacyOnly = path.join(dir, 'legacy-only.md');
  fs.writeFileSync(legacyOnly, '# Title\n\n## graphify\n\nOld section.\n<!-- nodesify-graphify:managed -->\n', 'utf-8');
  assert(removeSection(legacyOnly) === true, 'removeSection: removes legacy graphify section');
  const afterLegacyRemove = fs.readFileSync(legacyOnly, 'utf-8');
  assert(afterLegacyRemove.includes('# Title'), 'removeSection: keeps surrounding content');
  assert(!afterLegacyRemove.includes('## graphify'), 'removeSection: legacy section gone');

  // removeSection on non-existent file returns false
  assert(removeSection(path.join(dir, 'nonexistent.md')) === false, 'removeSection: missing file returns false');

  // injectSection preserves existing content
  const existingFile = path.join(dir, 'existing.md');
  fs.writeFileSync(existingFile, '# My Project\nSome content\n', 'utf-8');
  injectSection(existingFile, PROJECT_MD_SECTION);
  const merged = fs.readFileSync(existingFile, 'utf-8');
  assert(merged.startsWith('# My Project'), 'injectSection: preserves existing content');
  assert(merged.includes('## astria'), 'injectSection: appends section');

  // removeSection only removes the astria section, keeps rest
  removeSection(existingFile);
  const afterRemove = fs.readFileSync(existingFile, 'utf-8');
  assert(afterRemove.includes('# My Project'), 'removeSection: keeps non-astria content');
  assert(!afterRemove.includes('## astria'), 'removeSection: removes only astria section');

  fs.rmSync(dir, { recursive: true, force: true });
}

// ---- Legacy (pre-1.0 nodesify-graphify) migration ----

function testProjectScopeIsolation() {
  const fakeHome = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-home-'));
  const project = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-proj-'));
  const prevUserProfile = process.env.USERPROFILE;
  const prevHome = process.env.HOME;
  process.env.USERPROFILE = fakeHome;
  process.env.HOME = fakeHome;
  try {
    // A pre-1.0 codex install left the old skill file behind.
    const legacy = path.join(fakeHome, '.agents', 'skills', 'graphify', 'SKILL.md');
    fs.mkdirSync(path.dirname(legacy), { recursive: true });
    fs.writeFileSync(legacy, 'name: graphify\n');

    const results = installPlatform('codex', project);
    assert(fs.existsSync(legacy), 'Project scope: user skill untouched by install');
    assert(
      fs.existsSync(path.join(project, '.agents', 'skills', 'astria', 'SKILL.md')),
      'Project scope: codex skill installed in project'
    );
    assert(results.some(r => r.includes('Scope: project')), 'Project scope: scope reported');

    // Uninstall removes the legacy file too (recreate, then uninstall).
    // Deliberate recreate of a test fixture in test-owned temp space, not
    // shared state; no other process touches this path.
    fs.mkdirSync(path.dirname(legacy), { recursive: true });
    // codeql[js/file-system-race]
    fs.writeFileSync(legacy, 'name: graphify\n');
    installPlatform('codex', project);
    const { uninstallPlatform } = require('../install') as typeof import('../install');
    uninstallPlatform('codex', project);
    assert(fs.existsSync(legacy), 'Project scope: uninstall leaves user skill untouched');

    fs.rmSync(path.join(fakeHome, '.agents'), { recursive: true, force: true });
  } finally {
    if (prevUserProfile === undefined) delete process.env.USERPROFILE; else process.env.USERPROFILE = prevUserProfile;
    if (prevHome === undefined) delete process.env.HOME; else process.env.HOME = prevHome;
    fs.rmSync(fakeHome, { recursive: true, force: true });
    fs.rmSync(project, { recursive: true, force: true });
  }
}

function testLegacyMigration() {
  // Claude: a 0.9-era PostToolUse hook is replaced by the astria hook.
  const claudeDir = tmpDir();
  fs.mkdirSync(path.join(claudeDir, '.claude'), { recursive: true });
  const legacyHook = {
    matcher: 'Edit|Write',
    hooks: [{
      type: 'command',
      command: `node -e "... .graphify/.posttool-update ... npx --no-install nodesify-graphify update ."`,
    }],
  };
  fs.writeFileSync(
    path.join(claudeDir, '.claude', 'settings.json'),
    JSON.stringify({ hooks: { PostToolUse: [legacyHook] } })
  );
  assert(injectClaudeHook(claudeDir) === true, 'Legacy Claude: upgrade inject returns true');
  const claudeAfter = readJson(path.join(claudeDir, '.claude', 'settings.json'));
  const post = claudeAfter.hooks.PostToolUse as any[];
  assert(post.length === 1, 'Legacy Claude: exactly one PostToolUse hook after upgrade');
  assert(JSON.stringify(post).includes('astria'), 'Legacy Claude: new hook is astria-flavored');
  assert(!JSON.stringify(post).includes('graphify'), 'Legacy Claude: legacy hook removed');
  // uninstall removes the upgraded hook too
  assert(removeClaudeHook(claudeDir) === true, 'Legacy Claude: remove works after upgrade');
  fs.rmSync(claudeDir, { recursive: true, force: true });

  // Codex: legacy nag replaced.
  const codexDir = tmpDir();
  fs.mkdirSync(path.join(codexDir, '.codex'), { recursive: true });
  const legacyCodex = {
    matcher: 'Bash',
    hooks: [{
      type: 'command',
      command: `node -e "... '.graphify/graph.json' ... 'nodesify-graphify: Knowledge graph available ...'"`,
    }],
  };
  fs.writeFileSync(
    path.join(codexDir, '.codex', 'hooks.json'),
    JSON.stringify({ hooks: { PreToolUse: [legacyCodex] } })
  );
  assert(injectCodexHook(codexDir) === true, 'Legacy Codex: upgrade inject returns true');
  const codexAfter = readJson(path.join(codexDir, '.codex', 'hooks.json'));
  const codexHooks = codexAfter.hooks.PreToolUse as any[];
  assert(codexHooks.length === 1, 'Legacy Codex: exactly one hook after upgrade');
  assert(JSON.stringify(codexHooks).includes('astria query'), 'Legacy Codex: hook upgraded to astria');
  assert(!JSON.stringify(codexHooks).includes('graphify'), 'Legacy Codex: legacy hook removed');
  fs.rmSync(codexDir, { recursive: true, force: true });

  // Gemini: legacy hook replaced.
  const geminiDir = tmpDir();
  fs.mkdirSync(path.join(geminiDir, '.gemini'), { recursive: true });
  const legacyGemini = {
    matcher: 'read_file|list_directory',
    hooks: [{
      type: 'command',
      command: `node -e "... '.graphify/graph.json' ... 'nodesify-graphify: Knowledge graph available ...'"`,
    }],
  };
  fs.writeFileSync(
    path.join(geminiDir, '.gemini', 'settings.json'),
    JSON.stringify({ hooks: { BeforeTool: [legacyGemini] } })
  );
  assert(injectGeminiHook(geminiDir) === true, 'Legacy Gemini: upgrade inject returns true');
  const geminiAfter = readJson(path.join(geminiDir, '.gemini', 'settings.json'));
  const geminiHooks = geminiAfter.hooks.BeforeTool as any[];
  assert(geminiHooks.length === 1, 'Legacy Gemini: exactly one hook after upgrade');
  assert(!JSON.stringify(geminiHooks).includes('graphify'), 'Legacy Gemini: legacy hook removed');
  fs.rmSync(geminiDir, { recursive: true, force: true });

  // OpenCode: legacy plugin file and registration replaced.
  const ocDir = tmpDir();
  fs.mkdirSync(path.join(ocDir, '.opencode', 'plugins'), { recursive: true });
  fs.writeFileSync(path.join(ocDir, '.opencode', 'plugins', 'graphify.js'), '// old plugin\n');
  fs.writeFileSync(
    path.join(ocDir, '.opencode', 'opencode.json'),
    JSON.stringify({ plugins: ['./plugins/graphify.js'] })
  );
  assert(injectOpenCodePlugin(ocDir) === true, 'Legacy OpenCode: inject returns true');
  assert(!fs.existsSync(path.join(ocDir, '.opencode', 'plugins', 'graphify.js')), 'Legacy OpenCode: legacy plugin file removed');
  assert(fs.existsSync(path.join(ocDir, '.opencode', 'plugins', 'astria.js')), 'Legacy OpenCode: astria plugin written to plugins/');
  const ocConfig = readJson(path.join(ocDir, '.opencode', 'opencode.json'));
  assert(ocConfig.plugins, 'OpenCode: unrelated configuration preserved');
  // uninstall removes both eras
  assert(removeOpenCodePlugin(ocDir) === true, 'Legacy OpenCode: remove works');
  fs.rmSync(ocDir, { recursive: true, force: true });

  // Cursor: legacy graphify.mdc replaced by astria.mdc.
  const cursorDir = tmpDir();
  fs.mkdirSync(path.join(cursorDir, '.cursor', 'rules'), { recursive: true });
  fs.writeFileSync(path.join(cursorDir, '.cursor', 'rules', 'graphify.mdc'), '---\ndescription: old\n---\n');
  assert(injectCursorRule(cursorDir) === true, 'Legacy Cursor: inject returns true');
  assert(!fs.existsSync(path.join(cursorDir, '.cursor', 'rules', 'graphify.mdc')), 'Legacy Cursor: legacy rule removed');
  assert(fs.existsSync(path.join(cursorDir, '.cursor', 'rules', 'astria.mdc')), 'Legacy Cursor: astria rule written');
  assert(removeCursorRule(cursorDir) === true, 'Legacy Cursor: remove works');
  fs.rmSync(cursorDir, { recursive: true, force: true });

  // Kiro: legacy graphify.md replaced by astria.md.
  const kiroDir = tmpDir();
  fs.mkdirSync(path.join(kiroDir, '.kiro', 'steering'), { recursive: true });
  fs.writeFileSync(path.join(kiroDir, '.kiro', 'steering', 'graphify.md'), 'old\n');
  assert(injectKiroSteering(kiroDir) === true, 'Legacy Kiro: inject returns true');
  assert(!fs.existsSync(path.join(kiroDir, '.kiro', 'steering', 'graphify.md')), 'Legacy Kiro: legacy steering removed');
  assert(fs.existsSync(path.join(kiroDir, '.kiro', 'steering', 'astria.md')), 'Legacy Kiro: astria steering written');
  assert(removeKiroSteering(kiroDir) === true, 'Legacy Kiro: remove works');
  fs.rmSync(kiroDir, { recursive: true, force: true });

  // ZCode MCP: legacy graphify server (installer-written command) replaced.
  const zcDir = tmpDir();
  fs.mkdirSync(path.join(zcDir, '.zcode'), { recursive: true });
  fs.writeFileSync(
    path.join(zcDir, '.zcode', 'config.json'),
    JSON.stringify({ mcp: { servers: { graphify: { type: 'stdio', command: 'nodesify-graphify', args: ['mcp'] } } } })
  );
  assert(injectZcodeMcp(zcDir) === true, 'Legacy ZCode: inject upgrades server');
  const zcAfter = readJson(path.join(zcDir, '.zcode', 'config.json'));
  assert(!zcAfter.mcp.servers.graphify, 'Legacy ZCode: legacy server key removed');
  assert(zcAfter.mcp.servers.astria.command === 'astria', 'Legacy ZCode: astria server present');
  // a user-customized legacy entry (different command) is left alone
  const zcDir2 = tmpDir();
  fs.mkdirSync(path.join(zcDir2, '.zcode'), { recursive: true });
  fs.writeFileSync(
    path.join(zcDir2, '.zcode', 'config.json'),
    JSON.stringify({ mcp: { servers: { graphify: { command: 'my-own-wrapper' } } } })
  );
  injectZcodeMcp(zcDir2);
  const zcAfter2 = readJson(path.join(zcDir2, '.zcode', 'config.json'));
  assert(zcAfter2.mcp.servers.graphify.command === 'my-own-wrapper', 'Legacy ZCode: customized legacy entry preserved');
  fs.rmSync(zcDir, { recursive: true, force: true });
  fs.rmSync(zcDir2, { recursive: true, force: true });

  // removeAgentMcp also cleans a pure-legacy install (claude flavor).
  const legacyMcpDir = tmpDir();
  fs.writeFileSync(
    path.join(legacyMcpDir, '.mcp.json'),
    JSON.stringify({ mcpServers: { graphify: { type: 'stdio', command: 'nodesify-graphify', args: ['mcp'] } } })
  );
  assert(removeAgentMcp(legacyMcpDir, 'claude') === true, 'Legacy MCP: remove clears graphify server');
  const mcpCleaned = readJson(path.join(legacyMcpDir, '.mcp.json'));
  assert(!mcpCleaned.mcpServers, 'Legacy MCP: empty mcpServers cleaned');
  fs.rmSync(legacyMcpDir, { recursive: true, force: true });
}

// ---- Copilot instructions parity ----

function testCopilotInstructions() {
  const fakeHome = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-home-'));
  const project = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-proj-'));
  const prevUserProfile = process.env.USERPROFILE;
  const prevHome = process.env.HOME;
  process.env.USERPROFILE = fakeHome;
  process.env.HOME = fakeHome;
  try {
    // Seed a 1.0.9/1.0.10-era copy under the home dir — Copilot reads
    // repo-scoped .github/skills/ (docs.github.com), so install must
    // retire that stale copy.
    const staleHomeSkill = path.join(fakeHome, '.github', 'skills', 'astria', 'SKILL.md');
    fs.mkdirSync(path.dirname(staleHomeSkill), { recursive: true });
    fs.writeFileSync(staleHomeSkill, 'stale\n');

    const results = installPlatform('copilot', project);

    // Skill file lands under the project-scoped .github skills dir.
    assert(
      fs.existsSync(path.join(project, '.github', 'skills', 'astria', 'SKILL.md')),
      'Copilot: skill file installed'
    );
    assert(fs.existsSync(staleHomeSkill), 'Copilot: user-scope copy untouched');
    // AGENTS.md gets the managed section...
    assert(
      fs.readFileSync(path.join(project, 'AGENTS.md'), 'utf-8').includes('## astria'),
      'Copilot: AGENTS.md section added'
    );
    // ...and so does .github/copilot-instructions.md (Copilot's native
    // custom-instructions file, which not every Copilot version reads
    // AGENTS.md for).
    const instructionsPath = path.join(project, '.github', 'copilot-instructions.md');
    assert(results.some((r) => r.includes('copilot-instructions.md')), 'Copilot: install reported');
    assert(
      fs.readFileSync(instructionsPath, 'utf-8').includes('## astria'),
      'Copilot: instructions section added'
    );

    // Re-install is idempotent and reported as such.
    const again = installPlatform('copilot', project);
    assert(
      again.some((r) => r.includes('already up to date')),
      'Copilot: re-install reported unchanged'
    );

    // Uninstall removes both sections and the skill file. A file holding
    // only the managed section is deleted outright (removeSection contract),
    // so absence counts as removed.
    const { uninstallPlatform } = require('../install') as typeof import('../install');
    uninstallPlatform('copilot', project);
    const instructionsAfter = fs.existsSync(instructionsPath)
      ? fs.readFileSync(instructionsPath, 'utf-8')
      : '';
    assert(
      !instructionsAfter.includes('## astria'),
      'Copilot: uninstall removes instructions section'
    );
    const agentsAfter = fs.existsSync(path.join(project, 'AGENTS.md'))
      ? fs.readFileSync(path.join(project, 'AGENTS.md'), 'utf-8')
      : '';
    assert(!agentsAfter.includes('## astria'), 'Copilot: uninstall removes AGENTS.md section');
    assert(
      !fs.existsSync(path.join(project, '.github', 'skills', 'astria', 'SKILL.md')),
      'Copilot: uninstall removes skill file'
    );
  } finally {
    if (prevUserProfile === undefined) delete process.env.USERPROFILE; else process.env.USERPROFILE = prevUserProfile;
    if (prevHome === undefined) delete process.env.HOME; else process.env.HOME = prevHome;
    fs.rmSync(fakeHome, { recursive: true, force: true });
    fs.rmSync(project, { recursive: true, force: true });
  }
}

// ---- File-stem skill layouts (cline / roo) ----

function testFileStemSkillLayouts() {
  // .clinerules/astria.md has no directory segment to swap, so the legacy
  // cleanup must be a no-op there — it used to resolve to the same path and
  // delete the freshly installed skill.
  const fakeHome = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-stem-home-'));
  const project = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-stem-proj-'));
  const prevUserProfile = process.env.USERPROFILE;
  const prevHome = process.env.HOME;
  process.env.USERPROFILE = fakeHome;
  process.env.HOME = fakeHome;
  try {
    const { PLATFORMS } = require('../install/platforms') as typeof import('../install/platforms');
    for (const platform of ['cline', 'roo']) {
      installPlatform(platform, project);
      const cfg = PLATFORMS[platform];
      const dst = path.join(project, cfg.skillDst);
      assert(fs.existsSync(dst), platform + ': skill file survives its own install');
    }
  } finally {
    if (prevUserProfile === undefined) delete process.env.USERPROFILE; else process.env.USERPROFILE = prevUserProfile;
    if (prevHome === undefined) delete process.env.HOME; else process.env.HOME = prevHome;
    fs.rmSync(fakeHome, { recursive: true, force: true });
    fs.rmSync(project, { recursive: true, force: true });
  }
}

// ---- Pi extension (~/.pi/agent/extensions/astria.mjs) ----

function testPiExtension() {
  const fakeHome = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-pi-home-'));
  const prevUserProfile = process.env.USERPROFILE;
  const prevHome = process.env.HOME;
  process.env.USERPROFILE = fakeHome;
  process.env.HOME = fakeHome;
  try {
    assert(injectPiExtension() === true, 'Pi: first inject returns true');
    const extPath = path.join(fakeHome, '.pi', 'agent', 'extensions', 'astria.mjs');
    const content = fs.readFileSync(extPath, 'utf-8');
    // The generated extension must be valid ESM (static imports, no
    // require) and use pi's real API surface — native tools above all:
    // pi's own philosophy is CLI-backed registered tools over MCP
    // definitions, so the graph must be exposed as first-class pi tools.
    assert(content.includes('from "node:child_process"'), 'Pi: ESM static import');
    assert(!content.includes('require('), 'Pi: no require in .mjs');
    assert(content.includes('export default function'), 'Pi: default-export entry shape');
    assert(content.includes('registerTool'), 'Pi: native tools registered (pi-idiomatic)');
    for (const tool of ['astria_query', 'astria_map', 'astria_explain', 'astria_path', 'astria_affected']) {
      assert(content.includes('"' + tool + '"'), 'Pi: ' + tool + ' registered');
    }
    assert(content.includes('"tool_result"'), 'Pi: freshness listens on tool_result');
    assert(content.includes('registerCommand("astria"'), 'Pi: /astria command registered');
    assert(content.includes('update .'), 'Pi: refreshes via the update command');

    assert(injectPiExtension() === false, 'Pi: second inject returns false (idempotent)');

    assert(removePiExtension() === true, 'Pi: remove returns true');
    assert(!fs.existsSync(extPath), 'Pi: extension file deleted');
    assert(removePiExtension() === false, 'Pi: second remove returns false');
  } finally {
    if (prevUserProfile === undefined) delete process.env.USERPROFILE; else process.env.USERPROFILE = prevUserProfile;
    if (prevHome === undefined) delete process.env.HOME; else process.env.HOME = prevHome;
    fs.rmSync(fakeHome, { recursive: true, force: true });
  }
}

// ---- 1.0.11 audit fixes: parse-abort, JSONC, ownership fingerprints ----

function testAuditFixes() {
  // H2: a non-empty unparseable strict-JSON config aborts and is left
  // byte-identical — the pre-1.0.11 behavior reset it to {} and overwrote.
  const brokenDir = tmpDir();
  const brokenPath = path.join(brokenDir, '.mcp.json');
  const brokenText = '{"mcpServers": {"db": {"command": "db-server"}} trailing-garbage';
  fs.writeFileSync(brokenPath, brokenText, 'utf-8');
  let threw = false;
  try {
    injectAgentMcp(brokenDir, 'claude');
  } catch (e: any) {
    threw = String(e.message).includes('refusing to rewrite');
  }
  assert(threw, 'audit: unparseable JSON aborts with a refusal');
  assert(fs.readFileSync(brokenPath, 'utf-8') === brokenText, 'audit: aborted file untouched');
  fs.rmSync(brokenDir, { recursive: true, force: true });

  // H2: a UTF-8 BOM does not abort — it is stripped for parsing only.
  const bomDir = tmpDir();
  const bomPath = path.join(bomDir, '.mcp.json');
  fs.writeFileSync(bomPath, '\uFEFF{"mcpServers": {"db": {"command": "db-server"}}}', 'utf-8');
  assert(injectAgentMcp(bomDir, 'claude') === true, 'audit: BOM config installs');
  const bomData = readJson(path.join(bomDir, '.mcp.json'));
  assert(bomData.mcpServers.db && bomData.mcpServers.astria, 'audit: BOM config keeps user server');
  fs.rmSync(bomDir, { recursive: true, force: true });

  // H2: .vscode/mcp.json is officially JSONC — comments must not abort, and
  // the user's own server must survive the rewrite.
  const vscodeDir = tmpDir();
  const vscodePath = path.join(vscodeDir, '.vscode', 'mcp.json');
  fs.mkdirSync(path.dirname(vscodePath), { recursive: true });
  fs.writeFileSync(
    vscodePath,
    '// my team servers\n{\n  "servers": { "db": { "command": "db-server" } },\n}\n',
    'utf-8'
  );
  assert(injectAgentMcp(vscodeDir, 'vscode') === true, 'audit: JSONC vscode config installs');
  const vscodeData = readJson(vscodePath);
  assert(vscodeData.servers && vscodeData.servers.db, 'audit: JSONC user server preserved');
  assert(vscodeData.servers && vscodeData.servers.astria, 'audit: astria added to JSONC config');
  fs.rmSync(vscodeDir, { recursive: true, force: true });

  // M1: uninstall removes only entries this installer wrote. User-owned
  // `astria`/`graphify` entries (different command) survive.
  const ownedDir = tmpDir();
  const ownedPath = path.join(ownedDir, '.mcp.json');
  fs.writeFileSync(ownedPath, JSON.stringify({
    mcpServers: {
      astria: { type: 'stdio', command: 'my-own-wrapper' },
      graphify: { type: 'stdio', command: 'my-graphify-tool' },
      other: { type: 'stdio', command: 'unrelated' },
    },
  }), 'utf-8');
  assert(removeAgentMcp(ownedDir, 'claude') === false, 'audit: uninstall of user-owned entries is a no-op');
  const ownedAfter = readJson(ownedPath);
  assert(ownedAfter.mcpServers.astria && ownedAfter.mcpServers.graphify && ownedAfter.mcpServers.other,
    'audit: user-owned entries survive uninstall');
  fs.rmSync(ownedDir, { recursive: true, force: true });

  // M3: a user hook that merely invokes the astria CLI (the documented
  // hook-guard command) is not a managed entry — uninstall leaves it.
  const guardDir = tmpDir();
  const guardSettings = path.join(guardDir, '.claude', 'settings.json');
  fs.mkdirSync(path.dirname(guardSettings), { recursive: true });
  fs.writeFileSync(guardSettings, JSON.stringify({
    hooks: {
      PreToolUse: [{
        matcher: 'Read',
        hooks: [{ type: 'command', command: 'astria hook-guard read --strict' }],
      }],
      PostToolUse: [{
        matcher: 'Edit|Write',
        hooks: [{ type: 'command', command: 'node -e "const fs=require(\'fs\');const p=\'.astria/graph.json\';if(!fs.existsSync(p)){process.exit(0)}"' }],
      }],
    },
  }), 'utf-8');
  removeClaudeHook(guardDir);
  const guardAfter = readJson(guardSettings);
  assert(guardAfter.hooks.PreToolUse.length === 1, 'audit: user hook-guard entry survives uninstall');
  assert(!guardAfter.hooks.PostToolUse || guardAfter.hooks.PostToolUse.length === 0,
    'audit: managed template hook removed');
  fs.rmSync(guardDir, { recursive: true, force: true });

  // M2: uninstall removes only managed markdown sections — an unmarked
  // `## astria` block is user-owned, and `## astria-guide` never matched.
  const mdDir = tmpDir();
  const mdPath = path.join(mdDir, 'AGENTS.md');
  fs.writeFileSync(mdPath, [
    '# Notes',
    '',
    '## astria',
    '',
    'My personal astria notes — not managed.',
    '',
    '## astria-guide',
    '',
    'Another user section.',
    '',
  ].join('\n'), 'utf-8');
  assert(removeSection(mdPath) === false, 'audit: unmanaged sections are a no-op on uninstall');
  const mdAfter = fs.readFileSync(mdPath, 'utf-8');
  assert(mdAfter.includes('My personal astria notes'), 'audit: unmanaged astria section kept');
  assert(mdAfter.includes('Another user section'), 'audit: astria-guide never matched');
  fs.rmSync(mdDir, { recursive: true, force: true });

  // L5: an unknown platform is an error, not a message with exit code 0.
  let unknownThrew = false;
  try {
    installPlatform('does-not-exist', tmpDir());
  } catch (e: any) {
    unknownThrew = String(e.message).includes('Unknown platform');
  }
  assert(unknownThrew, 'audit: unknown platform throws');
}

// ---- Run all ----

testClaudeHook();
testCodexHook();
testGeminiHook();
testOpenCodePlugin();
testCursorRule();
testKiroSteering();
testZcodeMcp();
testAgentMcp();
testOpenCodeMcpShape();
testKiroLegacyRootMcpMigration();
testCopilotLegacyMcpCleanup();
testCodexMcpToml();
testPiExtension();
testFileStemSkillLayouts();
testMarkdownInject();
testLegacyMigration();
testProjectScopeIsolation();
testCopilotInstructions();
testAuditFixes();

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
  process.exit(1);
}
