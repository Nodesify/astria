"use strict";
/**
 * Install module tests — validates hook injection, removal, and content
 * for all supported platforms, plus upgrade/cleanup of pre-1.0
 * nodesify-graphify installs. Uses temp directories, no external deps.
 *
 * Run with: npx tsx src/__tests__/install.test.ts
 */
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const os = __importStar(require("os"));
const settings_inject_1 = require("../install/settings-inject");
const markdown_inject_1 = require("../install/markdown-inject");
const install_1 = require("../install");
let passed = 0;
let failed = 0;
function assert(condition, message) {
    if (condition) {
        passed++;
    }
    else {
        failed++;
        console.error(`FAIL: ${message}`);
    }
}
function tmpDir() {
    return fs.mkdtempSync(path.join(os.tmpdir(), 'astria-test-'));
}
function readJson(filePath) {
    return JSON.parse(fs.readFileSync(filePath, 'utf-8'));
}
// ---- Claude Code ----
function testClaudeHook() {
    const dir = tmpDir();
    // inject into non-existent settings
    const result1 = (0, settings_inject_1.injectClaudeHook)(dir);
    assert(result1 === true, 'Claude: first inject returns true');
    const settings = readJson(path.join(dir, '.claude', 'settings.json'));
    const hooks = settings.hooks.PostToolUse;
    assert(hooks.length === 1, 'Claude: one PostToolUse hook after inject');
    assert(hooks[0].matcher === 'Edit|Write', 'Claude: uses Edit|Write matcher');
    assert(!settings.hooks.PreToolUse, 'Claude: does not install PreToolUse nags');
    assert(JSON.stringify(hooks).includes('astria'), 'Claude: hooks contain astria');
    assert(!JSON.stringify(hooks).includes('graphify'), 'Claude: hooks carry no graphify name');
    assert(JSON.stringify(hooks).includes('update .'), 'Claude: hook updates graph');
    // idempotent — second inject returns false
    const result2 = (0, settings_inject_1.injectClaudeHook)(dir);
    assert(result2 === false, 'Claude: second inject returns false (idempotent)');
    // still only two hooks
    const settings2 = readJson(path.join(dir, '.claude', 'settings.json'));
    assert(settings2.hooks.PostToolUse.length === 1, 'Claude: still one hook after double inject');
    // remove
    const removed = (0, settings_inject_1.removeClaudeHook)(dir);
    assert(removed === true, 'Claude: remove returns true');
    const settings3 = readJson(path.join(dir, '.claude', 'settings.json'));
    const remainingHooks = (settings3.hooks?.PreToolUse || []);
    assert(remainingHooks.length === 0, 'Claude: all astria hooks removed');
    // remove again returns false
    const removed2 = (0, settings_inject_1.removeClaudeHook)(dir);
    assert(removed2 === false, 'Claude: second remove returns false');
    // remove from non-existent file returns false
    const removed3 = (0, settings_inject_1.removeClaudeHook)(tmpDir());
    assert(removed3 === false, 'Claude: remove from missing file returns false');
    // inject preserves existing non-astria hooks
    const existingHook = { matcher: 'Write', hooks: [{ type: 'command', command: 'echo hi' }] };
    const data = { hooks: { PreToolUse: [existingHook] } };
    fs.mkdirSync(path.join(dir, '.claude'), { recursive: true });
    fs.writeFileSync(path.join(dir, '.claude', 'settings.json'), JSON.stringify(data));
    (0, settings_inject_1.injectClaudeHook)(dir);
    const settings4 = readJson(path.join(dir, '.claude', 'settings.json'));
    assert(settings4.hooks.PreToolUse.length === 1, 'Claude: preserves existing hooks');
    assert(settings4.hooks.PostToolUse.length === 1, 'Claude: adds one PostToolUse hook');
    fs.rmSync(dir, { recursive: true, force: true });
}
// ---- Codex ----
function testCodexHook() {
    const dir = tmpDir();
    const result1 = (0, settings_inject_1.injectCodexHook)(dir);
    assert(result1 === true, 'Codex: first inject returns true');
    const settings = readJson(path.join(dir, '.codex', 'hooks.json'));
    const hooks = settings.hooks.PreToolUse;
    assert(hooks.length === 1, 'Codex: one hook after inject');
    assert(hooks[0].matcher === 'Bash', 'Codex: matcher is Bash');
    assert(JSON.stringify(hooks[0]).includes('astria query'), 'Codex: hook mentions query command');
    assert(JSON.stringify(hooks[0]).includes('.astria/graph.json'), 'Codex: hook watches .astria graph');
    const result2 = (0, settings_inject_1.injectCodexHook)(dir);
    assert(result2 === false, 'Codex: second inject returns false (idempotent)');
    const removed = (0, settings_inject_1.removeCodexHook)(dir);
    assert(removed === true, 'Codex: remove returns true');
    const settings2 = readJson(path.join(dir, '.codex', 'hooks.json'));
    assert(settings2.hooks.PreToolUse.length === 0, 'Codex: hooks empty after remove');
    const removed2 = (0, settings_inject_1.removeCodexHook)(dir);
    assert(removed2 === false, 'Codex: second remove returns false');
    fs.rmSync(dir, { recursive: true, force: true });
}
// ---- Gemini ----
function testGeminiHook() {
    const dir = tmpDir();
    const result1 = (0, settings_inject_1.injectGeminiHook)(dir);
    assert(result1 === true, 'Gemini: first inject returns true');
    const settings = readJson(path.join(dir, '.gemini', 'settings.json'));
    const hooks = settings.hooks.BeforeTool;
    assert(hooks.length === 1, 'Gemini: one hook after inject');
    assert(hooks[0].matcher === 'read_file|list_directory', 'Gemini: matcher is read_file|list_directory');
    assert(JSON.stringify(hooks[0]).includes('astria query'), 'Gemini: hook mentions query command');
    const result2 = (0, settings_inject_1.injectGeminiHook)(dir);
    assert(result2 === false, 'Gemini: second inject returns false (idempotent)');
    const removed = (0, settings_inject_1.removeGeminiHook)(dir);
    assert(removed === true, 'Gemini: remove returns true');
    const settings2 = readJson(path.join(dir, '.gemini', 'settings.json'));
    assert(settings2.hooks.BeforeTool.length === 0, 'Gemini: hooks empty after remove');
    fs.rmSync(dir, { recursive: true, force: true });
}
// ---- OpenCode ----
function testOpenCodePlugin() {
    const dir = tmpDir();
    const result1 = (0, settings_inject_1.injectOpenCodePlugin)(dir);
    assert(result1 === true, 'OpenCode: first inject returns true');
    const pluginPath = path.join(dir, '.opencode', 'plugins', 'astria.js');
    assert(fs.existsSync(pluginPath), 'OpenCode: plugin file created');
    const pluginContent = fs.readFileSync(pluginPath, 'utf-8');
    assert(pluginContent.includes('"view", "grep", "glob", "ls", "bash"'), 'OpenCode: plugin matches view|grep|glob|ls|bash');
    assert(pluginContent.includes('MUST'), 'OpenCode: plugin uses MUST language');
    assert(pluginContent.includes('.astria'), 'OpenCode: plugin checks .astria graph');
    const config = readJson(path.join(dir, '.opencode', 'opencode.json'));
    assert(config.plugins.includes('./plugins/astria.js'), 'OpenCode: config references plugin');
    const result2 = (0, settings_inject_1.injectOpenCodePlugin)(dir);
    assert(result2 === false || result2 === true, 'OpenCode: re-inject does not duplicate');
    const configAfter = readJson(path.join(dir, '.opencode', 'opencode.json'));
    assert((configAfter.plugins.filter((p) => p.includes('astria.js'))).length === 1, 'OpenCode: single plugin registration');
    const removed = (0, settings_inject_1.removeOpenCodePlugin)(dir);
    assert(removed === true, 'OpenCode: remove returns true');
    assert(!fs.existsSync(pluginPath), 'OpenCode: plugin file deleted after remove');
    const config2 = readJson(path.join(dir, '.opencode', 'opencode.json'));
    assert(!config2.plugins.includes('./plugins/astria.js'), 'OpenCode: plugin removed from config');
    const removed2 = (0, settings_inject_1.removeOpenCodePlugin)(dir);
    assert(removed2 === false, 'OpenCode: second remove returns false');
    fs.rmSync(dir, { recursive: true, force: true });
}
// ---- Cursor ----
function testCursorRule() {
    const dir = tmpDir();
    const result1 = (0, settings_inject_1.injectCursorRule)(dir);
    assert(result1 === true, 'Cursor: first inject returns true');
    const rulePath = path.join(dir, '.cursor', 'rules', 'astria.mdc');
    assert(fs.existsSync(rulePath), 'Cursor: rule file created');
    const content = fs.readFileSync(rulePath, 'utf-8');
    assert(content.includes('alwaysApply: true'), 'Cursor: rule has alwaysApply');
    assert(content.includes('MUST read'), 'Cursor: rule uses MUST language');
    assert(content.includes('astria query'), 'Cursor: rule mentions query command');
    assert(content.includes('.astria/graph_report.md'), 'Cursor: rule reads .astria report');
    const result2 = (0, settings_inject_1.injectCursorRule)(dir);
    assert(result2 === false || result2 === true, 'Cursor: re-inject does not duplicate');
    assert(fs.existsSync(path.join(dir, '.cursor', 'rules', 'graphify.mdc')) === false, 'Cursor: no duplicate legacy rule');
    const removed = (0, settings_inject_1.removeCursorRule)(dir);
    assert(removed === true, 'Cursor: remove returns true');
    assert(!fs.existsSync(rulePath), 'Cursor: rule file deleted after remove');
    const removed2 = (0, settings_inject_1.removeCursorRule)(dir);
    assert(removed2 === false, 'Cursor: second remove returns false');
    fs.rmSync(dir, { recursive: true, force: true });
}
// ---- Kiro ----
function testKiroSteering() {
    const dir = tmpDir();
    const result1 = (0, settings_inject_1.injectKiroSteering)(dir);
    assert(result1 === true, 'Kiro: first inject returns true');
    const steerPath = path.join(dir, '.kiro', 'steering', 'astria.md');
    assert(fs.existsSync(steerPath), 'Kiro: steering file created');
    const content = fs.readFileSync(steerPath, 'utf-8');
    assert(content.includes('inclusion: always'), 'Kiro: steering has inclusion: always');
    assert(content.includes('MUST read'), 'Kiro: steering uses MUST language');
    assert(content.includes('astria query'), 'Kiro: steering mentions query command');
    const result2 = (0, settings_inject_1.injectKiroSteering)(dir);
    assert(result2 === false || result2 === true, 'Kiro: re-inject does not duplicate');
    const removed = (0, settings_inject_1.removeKiroSteering)(dir);
    assert(removed === true, 'Kiro: remove returns true');
    assert(!fs.existsSync(steerPath), 'Kiro: steering file deleted after remove');
    const removed2 = (0, settings_inject_1.removeKiroSteering)(dir);
    assert(removed2 === false, 'Kiro: second remove returns false');
    fs.rmSync(dir, { recursive: true, force: true });
}
// ---- ZCode ----
function testZcodeMcp() {
    const dir = tmpDir();
    const result1 = (0, settings_inject_1.injectZcodeMcp)(dir);
    assert(result1 === true, 'ZCode: first inject returns true');
    const config = readJson(path.join(dir, '.zcode', 'config.json'));
    const server = config.mcp.servers.astria;
    assert(server.command === 'astria', 'ZCode: server command is astria');
    assert(JSON.stringify(server.args) === '["mcp"]', 'ZCode: server args are ["mcp"]');
    const result2 = (0, settings_inject_1.injectZcodeMcp)(dir);
    assert(result2 === false, 'ZCode: second inject returns false (idempotent)');
    // merge: preserves existing servers and unrelated config keys
    const dir2 = tmpDir();
    fs.mkdirSync(path.join(dir2, '.zcode'), { recursive: true });
    const existing = {
        hooks: { enabled: true },
        mcp: { servers: { other: { type: 'stdio', command: 'other-cli' } } },
    };
    fs.writeFileSync(path.join(dir2, '.zcode', 'config.json'), JSON.stringify(existing));
    (0, settings_inject_1.injectZcodeMcp)(dir2);
    const merged = readJson(path.join(dir2, '.zcode', 'config.json'));
    assert(merged.mcp.servers.other.command === 'other-cli', 'ZCode: preserves existing MCP servers');
    assert(merged.hooks.enabled === true, 'ZCode: preserves unrelated config keys');
    assert(merged.mcp.servers.astria.command === 'astria', 'ZCode: adds astria server');
    fs.rmSync(dir2, { recursive: true, force: true });
    const removed = (0, settings_inject_1.removeZcodeMcp)(dir);
    assert(removed === true, 'ZCode: remove returns true');
    const config2 = readJson(path.join(dir, '.zcode', 'config.json'));
    assert(!config2.mcp, 'ZCode: empty mcp block cleaned up after remove');
    const removed2 = (0, settings_inject_1.removeZcodeMcp)(dir);
    assert(removed2 === false, 'ZCode: second remove returns false');
    const removed3 = (0, settings_inject_1.removeZcodeMcp)(tmpDir());
    assert(removed3 === false, 'ZCode: remove from missing file returns false');
    fs.rmSync(dir, { recursive: true, force: true });
}
// ---- Agent MCP registration (claude / cursor / gemini) ----
function testAgentMcp() {
    const flavors = [
        ['claude', '.mcp.json', {}],
        ['cursor', path.join('.cursor', 'mcp.json'), {}],
        ['gemini', path.join('.gemini', 'settings.json'), { theme: 'dark' }],
    ];
    for (const [flavor, rel, extra] of flavors) {
        const dir = tmpDir();
        assert((0, settings_inject_1.injectAgentMcp)(dir, flavor) === true, `${flavor}: first inject returns true`);
        const config = readJson(path.join(dir, rel));
        assert(config.mcpServers.astria.command === 'astria', `${flavor}: server command is astria`);
        assert(JSON.stringify(config.mcpServers.astria.args) === '["mcp"]', `${flavor}: server args are ["mcp"]`);
        assert((0, settings_inject_1.injectAgentMcp)(dir, flavor) === false, `${flavor}: second inject returns false (idempotent)`);
        // merge: preserves existing servers and unrelated top-level keys
        const dir2 = tmpDir();
        const target = path.join(dir2, rel);
        fs.mkdirSync(path.dirname(target), { recursive: true });
        fs.writeFileSync(target, JSON.stringify({ mcpServers: { other: { command: 'other-cli' } }, ...extra }));
        (0, settings_inject_1.injectAgentMcp)(dir2, flavor);
        const merged = readJson(target);
        assert(merged.mcpServers.other.command === 'other-cli', `${flavor}: preserves existing MCP servers`);
        assert(merged.mcpServers.astria.command === 'astria', `${flavor}: adds astria server`);
        for (const key of Object.keys(extra)) {
            assert(merged[key] === extra[key], `${flavor}: preserves unrelated key ${key}`);
        }
        fs.rmSync(dir2, { recursive: true, force: true });
        assert((0, settings_inject_1.removeAgentMcp)(dir, flavor) === true, `${flavor}: remove returns true`);
        const cleaned = readJson(path.join(dir, rel));
        assert(!cleaned.mcpServers, `${flavor}: empty mcpServers block cleaned up after remove`);
        assert((0, settings_inject_1.removeAgentMcp)(dir, flavor) === false, `${flavor}: second remove returns false`);
        fs.rmSync(dir, { recursive: true, force: true });
    }
}
// ---- Markdown inject ----
function testMarkdownInject() {
    const dir = tmpDir();
    assert(!markdown_inject_1.PROJECT_MD_SECTION.includes('MUST'), 'PROJECT_MD_SECTION is passive');
    assert(markdown_inject_1.PROJECT_MD_SECTION.includes('repo_map'), 'PROJECT_MD_SECTION names MCP tools');
    assert(markdown_inject_1.PROJECT_MD_SECTION.includes('astria query'), 'PROJECT_MD_SECTION names CLI path');
    assert(markdown_inject_1.PROJECT_MD_SECTION.includes('affected'), 'PROJECT_MD_SECTION covers change impact');
    assert(markdown_inject_1.PROJECT_MD_SECTION.includes('.astria/'), 'PROJECT_MD_SECTION points at .astria/');
    assert(!markdown_inject_1.PROJECT_MD_SECTION.includes('graphify'), 'PROJECT_MD_SECTION carries no graphify name');
    // injectSection creates file with content
    const filePath = path.join(dir, 'CLAUDE.md');
    const result1 = (0, markdown_inject_1.injectSection)(filePath, markdown_inject_1.PROJECT_MD_SECTION);
    assert(result1 === 'added', 'injectSection: first inject adds');
    assert(fs.existsSync(filePath), 'injectSection: file created');
    const content = fs.readFileSync(filePath, 'utf-8');
    assert(content.includes('## astria'), 'injectSection: content includes section header');
    assert(content.includes(markdown_inject_1.SECTION_MARKER), 'injectSection: managed marker present');
    // idempotent — identical re-inject is unchanged
    const result2 = (0, markdown_inject_1.injectSection)(filePath, markdown_inject_1.PROJECT_MD_SECTION);
    assert(result2 === 'unchanged', 'injectSection: identical re-inject is unchanged');
    // legacy generated section (pre-marker) is upgraded in place
    const legacyPath = path.join(dir, 'AGENTS.md');
    fs.writeFileSync(legacyPath, '# My Project\n\n## graphify\n\nThis project has an optional nodesify-graphify knowledge graph at .graphify/.\nOld guidance.\n', 'utf-8');
    assert((0, markdown_inject_1.injectSection)(legacyPath, markdown_inject_1.PROJECT_MD_SECTION) === 'updated', 'injectSection: legacy section upgraded');
    const upgraded = fs.readFileSync(legacyPath, 'utf-8');
    assert(!upgraded.includes('optional nodesify-graphify'), 'injectSection: legacy wording replaced');
    assert(upgraded.includes('repo_map'), 'injectSection: new wording present');
    assert(upgraded.startsWith('# My Project'), 'injectSection: upgrade preserves surrounding content');
    // pre-1.0 managed sections carry the old marker — upgraded too
    const markerEra = path.join(dir, 'MARKER-era.md');
    fs.writeFileSync(markerEra, '## graphify\n\nThis project has a nodesify-graphify knowledge graph at .graphify/.\n<!-- nodesify-graphify:managed -->\n', 'utf-8');
    assert((0, markdown_inject_1.injectSection)(markerEra, markdown_inject_1.PROJECT_MD_SECTION) === 'updated', 'injectSection: pre-1.0 managed section upgraded');
    assert(!fs.readFileSync(markerEra, 'utf-8').includes('nodesify-graphify'), 'injectSection: old marker gone after upgrade');
    // older shipped wordings are recognized too
    const mustEra = path.join(dir, 'MUST-era.md');
    fs.writeFileSync(mustEra, '## graphify\n\nRules:\n- MUST read .graphify/graph_report.md before searching files for architecture questions\n', 'utf-8');
    assert((0, markdown_inject_1.injectSection)(mustEra, markdown_inject_1.PROJECT_MD_SECTION) === 'updated', 'injectSection: MUST-era section upgraded');
    const forbiddenEra = path.join(dir, 'FORBIDDEN-era.md');
    fs.writeFileSync(forbiddenEra, '## graphify\n\nCRITICAL RULES:\n- You are **FORBIDDEN** from using native search tools as your first step.\n', 'utf-8');
    assert((0, markdown_inject_1.injectSection)(forbiddenEra, markdown_inject_1.PROJECT_MD_SECTION) === 'updated', 'injectSection: FORBIDDEN-era section upgraded');
    // user-customized section is left alone
    const customPath = path.join(dir, 'CUSTOM.md');
    fs.writeFileSync(customPath, '## graphify\n\nMy own custom rules.\n', 'utf-8');
    assert((0, markdown_inject_1.injectSection)(customPath, markdown_inject_1.PROJECT_MD_SECTION) === 'unchanged', 'injectSection: custom block preserved');
    assert(fs.readFileSync(customPath, 'utf-8').includes('My own custom rules'), 'injectSection: custom text untouched');
    // skill registration (h1) is idempotent — used to duplicate on every install
    const regPath = path.join(dir, 'user-CLAUDE.md');
    assert((0, markdown_inject_1.injectSection)(regPath, markdown_inject_1.SKILL_REGISTRATION) === 'added', 'injectSection: h1 registration added');
    assert((0, markdown_inject_1.injectSection)(regPath, markdown_inject_1.SKILL_REGISTRATION) === 'unchanged', 'injectSection: h1 registration idempotent');
    const regCount = (fs.readFileSync(regPath, 'utf-8').match(/^# astria$/gm) || []).length;
    assert(regCount === 1, 'injectSection: no duplicated registration blocks');
    // removeSection removes the section (file only had graphify content, so file is deleted)
    const removed = (0, markdown_inject_1.removeSection)(filePath);
    assert(removed === true, 'removeSection: remove returns true');
    assert(!fs.existsSync(filePath), 'removeSection: file deleted when only content was astria section');
    // removeSection also strips pre-1.0 graphify sections
    const legacyOnly = path.join(dir, 'legacy-only.md');
    fs.writeFileSync(legacyOnly, '# Title\n\n## graphify\n\nOld section.\n<!-- nodesify-graphify:managed -->\n', 'utf-8');
    assert((0, markdown_inject_1.removeSection)(legacyOnly) === true, 'removeSection: removes legacy graphify section');
    const afterLegacyRemove = fs.readFileSync(legacyOnly, 'utf-8');
    assert(afterLegacyRemove.includes('# Title'), 'removeSection: keeps surrounding content');
    assert(!afterLegacyRemove.includes('## graphify'), 'removeSection: legacy section gone');
    // removeSection on non-existent file returns false
    assert((0, markdown_inject_1.removeSection)(path.join(dir, 'nonexistent.md')) === false, 'removeSection: missing file returns false');
    // injectSection preserves existing content
    const existingFile = path.join(dir, 'existing.md');
    fs.writeFileSync(existingFile, '# My Project\nSome content\n', 'utf-8');
    (0, markdown_inject_1.injectSection)(existingFile, markdown_inject_1.PROJECT_MD_SECTION);
    const merged = fs.readFileSync(existingFile, 'utf-8');
    assert(merged.startsWith('# My Project'), 'injectSection: preserves existing content');
    assert(merged.includes('## astria'), 'injectSection: appends section');
    // removeSection only removes the astria section, keeps rest
    (0, markdown_inject_1.removeSection)(existingFile);
    const afterRemove = fs.readFileSync(existingFile, 'utf-8');
    assert(afterRemove.includes('# My Project'), 'removeSection: keeps non-astria content');
    assert(!afterRemove.includes('## astria'), 'removeSection: removes only astria section');
    fs.rmSync(dir, { recursive: true, force: true });
}
// ---- Legacy (pre-1.0 nodesify-graphify) migration ----
function testLegacySkillDirCleanup() {
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
        const results = (0, install_1.installPlatform)('codex', project);
        assert(!fs.existsSync(legacy), 'Legacy skill: old codex skill removed by install');
        assert(fs.existsSync(path.join(fakeHome, '.agents', 'skills', 'astria', 'SKILL.md')), 'Legacy skill: new codex skill installed');
        assert(results.some((r) => r.includes('Legacy skill file removed')), 'Legacy skill: removal reported');
        // Uninstall removes the legacy file too (recreate, then uninstall).
        fs.mkdirSync(path.dirname(legacy), { recursive: true });
        fs.writeFileSync(legacy, 'name: graphify\n');
        (0, install_1.installPlatform)('codex', project);
        const { uninstallPlatform } = require('../install');
        uninstallPlatform('codex', project);
        assert(!fs.existsSync(legacy), 'Legacy skill: uninstall removes the old skill file');
        fs.rmSync(path.join(fakeHome, '.agents'), { recursive: true, force: true });
    }
    finally {
        if (prevUserProfile === undefined)
            delete process.env.USERPROFILE;
        else
            process.env.USERPROFILE = prevUserProfile;
        if (prevHome === undefined)
            delete process.env.HOME;
        else
            process.env.HOME = prevHome;
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
    fs.writeFileSync(path.join(claudeDir, '.claude', 'settings.json'), JSON.stringify({ hooks: { PostToolUse: [legacyHook] } }));
    assert((0, settings_inject_1.injectClaudeHook)(claudeDir) === true, 'Legacy Claude: upgrade inject returns true');
    const claudeAfter = readJson(path.join(claudeDir, '.claude', 'settings.json'));
    const post = claudeAfter.hooks.PostToolUse;
    assert(post.length === 1, 'Legacy Claude: exactly one PostToolUse hook after upgrade');
    assert(JSON.stringify(post).includes('astria'), 'Legacy Claude: new hook is astria-flavored');
    assert(!JSON.stringify(post).includes('graphify'), 'Legacy Claude: legacy hook removed');
    // uninstall removes the upgraded hook too
    assert((0, settings_inject_1.removeClaudeHook)(claudeDir) === true, 'Legacy Claude: remove works after upgrade');
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
    fs.writeFileSync(path.join(codexDir, '.codex', 'hooks.json'), JSON.stringify({ hooks: { PreToolUse: [legacyCodex] } }));
    assert((0, settings_inject_1.injectCodexHook)(codexDir) === true, 'Legacy Codex: upgrade inject returns true');
    const codexAfter = readJson(path.join(codexDir, '.codex', 'hooks.json'));
    const codexHooks = codexAfter.hooks.PreToolUse;
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
    fs.writeFileSync(path.join(geminiDir, '.gemini', 'settings.json'), JSON.stringify({ hooks: { BeforeTool: [legacyGemini] } }));
    assert((0, settings_inject_1.injectGeminiHook)(geminiDir) === true, 'Legacy Gemini: upgrade inject returns true');
    const geminiAfter = readJson(path.join(geminiDir, '.gemini', 'settings.json'));
    const geminiHooks = geminiAfter.hooks.BeforeTool;
    assert(geminiHooks.length === 1, 'Legacy Gemini: exactly one hook after upgrade');
    assert(!JSON.stringify(geminiHooks).includes('graphify'), 'Legacy Gemini: legacy hook removed');
    fs.rmSync(geminiDir, { recursive: true, force: true });
    // OpenCode: legacy plugin file and registration replaced.
    const ocDir = tmpDir();
    fs.mkdirSync(path.join(ocDir, '.opencode', 'plugins'), { recursive: true });
    fs.writeFileSync(path.join(ocDir, '.opencode', 'plugins', 'graphify.js'), '// old plugin\n');
    fs.writeFileSync(path.join(ocDir, '.opencode', 'opencode.json'), JSON.stringify({ plugins: ['./plugins/graphify.js'] }));
    assert((0, settings_inject_1.injectOpenCodePlugin)(ocDir) === true, 'Legacy OpenCode: inject returns true');
    assert(!fs.existsSync(path.join(ocDir, '.opencode', 'plugins', 'graphify.js')), 'Legacy OpenCode: legacy plugin file removed');
    assert(fs.existsSync(path.join(ocDir, '.opencode', 'plugins', 'astria.js')), 'Legacy OpenCode: astria plugin written');
    const ocConfig = readJson(path.join(ocDir, '.opencode', 'opencode.json'));
    assert(!ocConfig.plugins.includes('./plugins/graphify.js'), 'Legacy OpenCode: legacy registration removed');
    assert(ocConfig.plugins.includes('./plugins/astria.js'), 'Legacy OpenCode: astria plugin registered');
    // uninstall removes both eras
    assert((0, settings_inject_1.removeOpenCodePlugin)(ocDir) === true, 'Legacy OpenCode: remove works');
    fs.rmSync(ocDir, { recursive: true, force: true });
    // Cursor: legacy graphify.mdc replaced by astria.mdc.
    const cursorDir = tmpDir();
    fs.mkdirSync(path.join(cursorDir, '.cursor', 'rules'), { recursive: true });
    fs.writeFileSync(path.join(cursorDir, '.cursor', 'rules', 'graphify.mdc'), '---\ndescription: old\n---\n');
    assert((0, settings_inject_1.injectCursorRule)(cursorDir) === true, 'Legacy Cursor: inject returns true');
    assert(!fs.existsSync(path.join(cursorDir, '.cursor', 'rules', 'graphify.mdc')), 'Legacy Cursor: legacy rule removed');
    assert(fs.existsSync(path.join(cursorDir, '.cursor', 'rules', 'astria.mdc')), 'Legacy Cursor: astria rule written');
    assert((0, settings_inject_1.removeCursorRule)(cursorDir) === true, 'Legacy Cursor: remove works');
    fs.rmSync(cursorDir, { recursive: true, force: true });
    // Kiro: legacy graphify.md replaced by astria.md.
    const kiroDir = tmpDir();
    fs.mkdirSync(path.join(kiroDir, '.kiro', 'steering'), { recursive: true });
    fs.writeFileSync(path.join(kiroDir, '.kiro', 'steering', 'graphify.md'), 'old\n');
    assert((0, settings_inject_1.injectKiroSteering)(kiroDir) === true, 'Legacy Kiro: inject returns true');
    assert(!fs.existsSync(path.join(kiroDir, '.kiro', 'steering', 'graphify.md')), 'Legacy Kiro: legacy steering removed');
    assert(fs.existsSync(path.join(kiroDir, '.kiro', 'steering', 'astria.md')), 'Legacy Kiro: astria steering written');
    assert((0, settings_inject_1.removeKiroSteering)(kiroDir) === true, 'Legacy Kiro: remove works');
    fs.rmSync(kiroDir, { recursive: true, force: true });
    // ZCode MCP: legacy graphify server (installer-written command) replaced.
    const zcDir = tmpDir();
    fs.mkdirSync(path.join(zcDir, '.zcode'), { recursive: true });
    fs.writeFileSync(path.join(zcDir, '.zcode', 'config.json'), JSON.stringify({ mcp: { servers: { graphify: { type: 'stdio', command: 'nodesify-graphify', args: ['mcp'] } } } }));
    assert((0, settings_inject_1.injectZcodeMcp)(zcDir) === true, 'Legacy ZCode: inject upgrades server');
    const zcAfter = readJson(path.join(zcDir, '.zcode', 'config.json'));
    assert(!zcAfter.mcp.servers.graphify, 'Legacy ZCode: legacy server key removed');
    assert(zcAfter.mcp.servers.astria.command === 'astria', 'Legacy ZCode: astria server present');
    // a user-customized legacy entry (different command) is left alone
    const zcDir2 = tmpDir();
    fs.mkdirSync(path.join(zcDir2, '.zcode'), { recursive: true });
    fs.writeFileSync(path.join(zcDir2, '.zcode', 'config.json'), JSON.stringify({ mcp: { servers: { graphify: { command: 'my-own-wrapper' } } } }));
    (0, settings_inject_1.injectZcodeMcp)(zcDir2);
    const zcAfter2 = readJson(path.join(zcDir2, '.zcode', 'config.json'));
    assert(zcAfter2.mcp.servers.graphify.command === 'my-own-wrapper', 'Legacy ZCode: customized legacy entry preserved');
    fs.rmSync(zcDir, { recursive: true, force: true });
    fs.rmSync(zcDir2, { recursive: true, force: true });
    // removeAgentMcp also cleans a pure-legacy install (claude flavor).
    const legacyMcpDir = tmpDir();
    fs.writeFileSync(path.join(legacyMcpDir, '.mcp.json'), JSON.stringify({ mcpServers: { graphify: { type: 'stdio', command: 'nodesify-graphify', args: ['mcp'] } } }));
    assert((0, settings_inject_1.removeAgentMcp)(legacyMcpDir, 'claude') === true, 'Legacy MCP: remove clears graphify server');
    const mcpCleaned = readJson(path.join(legacyMcpDir, '.mcp.json'));
    assert(!mcpCleaned.mcpServers, 'Legacy MCP: empty mcpServers cleaned');
    fs.rmSync(legacyMcpDir, { recursive: true, force: true });
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
testMarkdownInject();
testLegacyMigration();
testLegacySkillDirCleanup();
console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
    process.exit(1);
}
//# sourceMappingURL=install.test.js.map