/**
 * Install module tests — validates hook injection, removal, and content
 * for all supported platforms. Uses temp directories, no external deps.
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
} from '../install/settings-inject';
import {
  injectSection, removeSection, PROJECT_MD_SECTION, SKILL_REGISTRATION,
} from '../install/markdown-inject';

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
  return fs.mkdtempSync(path.join(os.tmpdir(), 'graphify-test-'));
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
  assert(JSON.stringify(hooks).includes('graphify'), 'Claude: hooks contain graphify');
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
  assert(remainingHooks.length === 0, 'Claude: all graphify hooks removed');

  // remove again returns false
  const removed2 = removeClaudeHook(dir);
  assert(removed2 === false, 'Claude: second remove returns false');

  // remove from non-existent file returns false
  const removed3 = removeClaudeHook(tmpDir());
  assert(removed3 === false, 'Claude: remove from missing file returns false');

  // inject preserves existing non-graphify hooks
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
  assert(JSON.stringify(hooks[0]).includes('nodesify-graphify query'), 'Codex: hook mentions query command');

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
  assert(JSON.stringify(hooks[0]).includes('nodesify-graphify query'), 'Gemini: hook mentions query command');

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

  const pluginPath = path.join(dir, '.opencode', 'plugins', 'graphify.js');
  assert(fs.existsSync(pluginPath), 'OpenCode: plugin file created');
  const pluginContent = fs.readFileSync(pluginPath, 'utf-8');
  assert(pluginContent.includes('"view", "grep", "glob", "ls", "bash"'), 'OpenCode: plugin matches view|grep|glob|ls|bash');
  assert(pluginContent.includes('MUST'), 'OpenCode: plugin uses MUST language');

  const config = readJson(path.join(dir, '.opencode', 'opencode.json'));
  assert(config.plugins.includes('./plugins/graphify.js'), 'OpenCode: config references plugin');

  const result2 = injectOpenCodePlugin(dir);
  assert(result2 === false, 'OpenCode: second inject returns false (idempotent)');

  const removed = removeOpenCodePlugin(dir);
  assert(removed === true, 'OpenCode: remove returns true');
  assert(!fs.existsSync(pluginPath), 'OpenCode: plugin file deleted after remove');

  const config2 = readJson(path.join(dir, '.opencode', 'opencode.json'));
  assert(!config2.plugins.includes('./plugins/graphify.js'), 'OpenCode: plugin removed from config');

  const removed2 = removeOpenCodePlugin(dir);
  assert(removed2 === false, 'OpenCode: second remove returns false');

  fs.rmSync(dir, { recursive: true, force: true });
}

// ---- Cursor ----

function testCursorRule() {
  const dir = tmpDir();

  const result1 = injectCursorRule(dir);
  assert(result1 === true, 'Cursor: first inject returns true');

  const rulePath = path.join(dir, '.cursor', 'rules', 'graphify.mdc');
  assert(fs.existsSync(rulePath), 'Cursor: rule file created');
  const content = fs.readFileSync(rulePath, 'utf-8');
  assert(content.includes('alwaysApply: true'), 'Cursor: rule has alwaysApply');
  assert(content.includes('MUST read'), 'Cursor: rule uses MUST language');
  assert(content.includes('nodesify-graphify query'), 'Cursor: rule mentions query command');

  const result2 = injectCursorRule(dir);
  assert(result2 === false, 'Cursor: second inject returns false (idempotent)');

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

  const steerPath = path.join(dir, '.kiro', 'steering', 'graphify.md');
  assert(fs.existsSync(steerPath), 'Kiro: steering file created');
  const content = fs.readFileSync(steerPath, 'utf-8');
  assert(content.includes('inclusion: always'), 'Kiro: steering has inclusion: always');
  assert(content.includes('MUST read'), 'Kiro: steering uses MUST language');
  assert(content.includes('nodesify-graphify query'), 'Kiro: steering mentions query command');

  const result2 = injectKiroSteering(dir);
  assert(result2 === false, 'Kiro: second inject returns false (idempotent)');

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
  const server = config.mcp.servers.graphify;
  assert(server.command === 'nodesify-graphify', 'ZCode: server command is nodesify-graphify');
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
  assert(merged.mcp.servers.graphify.command === 'nodesify-graphify', 'ZCode: adds graphify server');
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

// ---- Markdown inject ----

function testMarkdownInject() {
  const dir = tmpDir();

  assert(!PROJECT_MD_SECTION.includes('MUST'), 'PROJECT_MD_SECTION is passive');
  assert(PROJECT_MD_SECTION.includes('repo_map'), 'PROJECT_MD_SECTION names MCP tools');
  assert(PROJECT_MD_SECTION.includes('nodesify-graphify query'), 'PROJECT_MD_SECTION names CLI path');
  assert(PROJECT_MD_SECTION.includes('affected'), 'PROJECT_MD_SECTION covers change impact');

  // injectSection creates file with content
  const filePath = path.join(dir, 'CLAUDE.md');
  const result1 = injectSection(filePath, PROJECT_MD_SECTION);
  assert(result1 === true, 'injectSection: first inject returns true');
  assert(fs.existsSync(filePath), 'injectSection: file created');
  const content = fs.readFileSync(filePath, 'utf-8');
  assert(content.includes('## graphify'), 'injectSection: content includes section header');

  // idempotent — second inject returns false
  const result2 = injectSection(filePath, PROJECT_MD_SECTION);
  assert(result2 === false, 'injectSection: second inject returns false (idempotent)');

  // removeSection removes the section (file only had graphify content, so file is deleted)
  const removed = removeSection(filePath);
  assert(removed === true, 'removeSection: remove returns true');
  assert(!fs.existsSync(filePath), 'removeSection: file deleted when only content was graphify section');

  // removeSection on non-existent file returns false
  assert(removeSection(path.join(dir, 'nonexistent.md')) === false, 'removeSection: missing file returns false');

  // injectSection preserves existing content
  const existingFile = path.join(dir, 'existing.md');
  fs.writeFileSync(existingFile, '# My Project\nSome content\n', 'utf-8');
  injectSection(existingFile, PROJECT_MD_SECTION);
  const merged = fs.readFileSync(existingFile, 'utf-8');
  assert(merged.startsWith('# My Project'), 'injectSection: preserves existing content');
  assert(merged.includes('## graphify'), 'injectSection: appends section');

  // removeSection only removes the graphify section, keeps rest
  removeSection(existingFile);
  const afterRemove = fs.readFileSync(existingFile, 'utf-8');
  assert(afterRemove.includes('# My Project'), 'removeSection: keeps non-graphify content');
  assert(!afterRemove.includes('## graphify'), 'removeSection: removes only graphify section');

  fs.rmSync(dir, { recursive: true, force: true });
}

// ---- Run all ----

testClaudeHook();
testCodexHook();
testGeminiHook();
testOpenCodePlugin();
testCursorRule();
testKiroSteering();
testZcodeMcp();
testMarkdownInject();

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
  process.exit(1);
}
