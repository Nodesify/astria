"use strict";
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
exports.installPlatform = installPlatform;
exports.uninstallPlatform = uninstallPlatform;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const os = __importStar(require("os"));
const platforms_1 = require("./platforms");
const markdown_inject_1 = require("./markdown-inject");
const settings_inject_1 = require("./settings-inject");
function sectionMessage(result, added, updated, unchanged) {
    return result === 'added' ? added : result === 'updated' ? updated : unchanged;
}
const MCP_LABELS = {
    zcode: { name: 'ZCode MCP server', file: '.zcode/config.json' },
    claude: { name: 'Claude MCP server', file: '.mcp.json' },
    cursor: { name: 'Cursor MCP server', file: '.cursor/mcp.json' },
    gemini: { name: 'Gemini MCP server', file: '.gemini/settings.json' },
};
function getSkillDir() {
    return path.resolve(__dirname, '..', '..', 'skills');
}
/// CLAUDE_CONFIG_DIR is read exactly once, at load, and sanitized to a
/// normalized absolute path without traversal segments (or undefined). No
/// other code path reads it, so env input can never reach a filesystem
/// write unvalidated.
const CLAUDE_CONFIG_DIR = (() => {
    const raw = process.env.CLAUDE_CONFIG_DIR;
    if (!raw)
        return undefined;
    const resolved = path.resolve(raw);
    return raw === resolved && !raw.includes('..') && path.isAbsolute(raw)
        ? resolved
        : undefined;
})();
function copySkillFile(platform, cfg) {
    const messages = [];
    if (!cfg.skillFile)
        return messages;
    // skillDst comes from the static platform table; refuse traversal
    // segments anyway so no runtime value can redirect the install target.
    if (cfg.skillDst.split(/[\\/]/).includes('..')) {
        messages.push(`Refusing unsafe skill destination: ${cfg.skillDst}`);
        return messages;
    }
    const src = path.join(getSkillDir(), cfg.skillFile);
    if (!fs.existsSync(src)) {
        messages.push(`Skill file not found: ${src}`);
        return messages;
    }
    const homeDir = os.homedir();
    const dst = path.join(homeDir, cfg.skillDst);
    if (platform === 'claude' && CLAUDE_CONFIG_DIR) {
        const overrideDst = path.join(CLAUDE_CONFIG_DIR, 'skills', 'astria', 'SKILL.md');
        copyFile(src, overrideDst);
        messages.push(`Skill file -> ${overrideDst}`);
        return messages;
    }
    copyFile(src, dst);
    messages.push(`Skill file -> ${dst}`);
    return messages;
}
function copyFile(src, dst) {
    // Sink guard: only normalized absolute destinations without traversal
    // segments may be written, regardless of how the caller derived them.
    if (!path.isAbsolute(dst) || dst.split(/[\\/]/).includes('..')) {
        return;
    }
    const dir = path.dirname(dst);
    if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
    }
    fs.copyFileSync(src, dst);
}
function writeInstallStamp(dir) {
    const stampPath = path.join(dir, '.astria_version');
    // Same guard as copyFile, applied to the exact value that is written:
    // only a normalized absolute path without traversal segments may reach
    // the filesystem, regardless of how the caller derived it.
    if (!path.isAbsolute(stampPath) || stampPath.split(/[\\/]/).includes('..')) {
        return;
    }
    try {
        fs.writeFileSync(stampPath, require('../../package.json').version + '\n', 'utf-8');
    }
    catch { /* ignore */ }
}
/// Pre-1.0 installs wrote skills under `skills/graphify/`. The path carries
/// exactly one `astria` segment (the skill dir name), so the legacy
/// destination is derivable by swapping that segment.
function legacySkillDst(cfg) {
    return cfg.skillDst.replace(/([\\/])astria([\\/])/, '$1graphify$2');
}
/// Removes the pre-1.0 skill file (and its now-empty folder) so a stale
/// graphify skill is never left behind after install or uninstall.
function removeLegacySkillFile(platform, cfg) {
    const messages = [];
    if (!cfg.skillFile)
        return messages;
    const targets = [path.join(os.homedir(), legacySkillDst(cfg))];
    if (platform === 'claude' && CLAUDE_CONFIG_DIR) {
        targets.push(path.join(CLAUDE_CONFIG_DIR, 'skills', 'graphify', 'SKILL.md'));
    }
    for (const legacy of targets) {
        const dir = path.dirname(legacy);
        const hadDir = fs.existsSync(dir);
        if (fs.existsSync(legacy)) {
            try {
                fs.unlinkSync(legacy);
                messages.push(`Legacy skill file removed: ${legacy}`);
            }
            catch { /* unreadable — leave it */ }
        }
        if (!hadDir)
            continue;
        // Install stamps from both eras; a folder holding only these is removed
        // outright so no stale skill directory lingers.
        for (const stamp of ['.astria_version', '.graphify_version']) {
            try {
                fs.unlinkSync(path.join(dir, stamp));
            }
            catch { /* absent */ }
        }
        try {
            fs.rmdirSync(dir);
        }
        catch { /* not empty — leave */ }
    }
    return messages;
}
function installPlatform(platform, projectDir) {
    const messages = [];
    if (platform === 'cursor') {
        if ((0, settings_inject_1.injectCursorRule)(projectDir)) {
            messages.push('Cursor rule -> .cursor/rules/astria.mdc');
        }
        else {
            messages.push('Cursor rule: already installed');
        }
        messages.push((0, settings_inject_1.injectAgentMcp)(projectDir, 'cursor')
            ? 'Cursor MCP server -> .cursor/mcp.json'
            : 'Cursor MCP server: already registered');
        return messages;
    }
    if (platform === 'kiro') {
        const cfg = platforms_1.PLATFORMS.kiro;
        messages.push(...copySkillFile('kiro', cfg));
        messages.push(...removeLegacySkillFile('kiro', cfg));
        if ((0, settings_inject_1.injectKiroSteering)(projectDir)) {
            messages.push('Kiro steering -> .kiro/steering/astria.md');
        }
        else {
            messages.push('Kiro steering: already installed');
        }
        return messages;
    }
    const cfg = platforms_1.PLATFORMS[platform];
    if (!cfg) {
        messages.push(`Unknown platform: ${platform}. Available: ${Object.keys(platforms_1.PLATFORMS).join(', ')}`);
        return messages;
    }
    messages.push(...copySkillFile(platform, cfg));
    messages.push(...removeLegacySkillFile(platform, cfg));
    if (cfg.claudeMd) {
        const claudeMdPath = path.join(os.homedir(), '.claude', 'CLAUDE.md');
        const registration = (0, markdown_inject_1.injectSection)(claudeMdPath, markdown_inject_1.SKILL_REGISTRATION);
        messages.push(sectionMessage(registration, 'User CLAUDE.md: skill registration added', 'User CLAUDE.md: skill registration updated', 'User CLAUDE.md: already registered'));
    }
    const projectMd = path.join(projectDir, 'CLAUDE.md');
    if (cfg.claudeMd || cfg.agentsMd || cfg.geminiMd) {
        if (cfg.claudeMd) {
            const result = (0, markdown_inject_1.injectSection)(projectMd, markdown_inject_1.PROJECT_MD_SECTION);
            messages.push(sectionMessage(result, 'Project CLAUDE.md: astria section added', 'Project CLAUDE.md: astria section updated', 'Project CLAUDE.md: astria section unchanged'));
        }
        if (cfg.agentsMd) {
            const agentsMd = path.join(projectDir, 'AGENTS.md');
            const result = (0, markdown_inject_1.injectSection)(agentsMd, markdown_inject_1.PROJECT_MD_SECTION);
            messages.push(sectionMessage(result, 'Project AGENTS.md: astria section added', 'Project AGENTS.md: astria section updated', 'Project AGENTS.md: astria section unchanged'));
        }
        if (cfg.geminiMd) {
            const geminiMd = path.join(projectDir, 'GEMINI.md');
            const result = (0, markdown_inject_1.injectSection)(geminiMd, markdown_inject_1.PROJECT_MD_SECTION);
            messages.push(sectionMessage(result, 'Project GEMINI.md: astria section added', 'Project GEMINI.md: astria section updated', 'Project GEMINI.md: astria section unchanged'));
        }
    }
    switch (cfg.settingsHook) {
        case 'claude':
            if ((0, settings_inject_1.injectClaudeHook)(projectDir)) {
                messages.push('Claude PreToolUse hook -> .claude/settings.json');
            }
            else {
                messages.push('Claude PreToolUse hook: already installed');
            }
            break;
        case 'codex':
            if ((0, settings_inject_1.injectCodexHook)(projectDir)) {
                messages.push('Codex PreToolUse hook -> .codex/hooks.json');
            }
            else {
                messages.push('Codex PreToolUse hook: already installed');
            }
            break;
        case 'gemini':
            if ((0, settings_inject_1.injectGeminiHook)(projectDir)) {
                messages.push('Gemini BeforeTool hook -> .gemini/settings.json');
            }
            else {
                messages.push('Gemini BeforeTool hook: already installed');
            }
            break;
        case 'opencode':
            if ((0, settings_inject_1.injectOpenCodePlugin)(projectDir)) {
                messages.push('OpenCode plugin -> .opencode/plugins/astria.js');
            }
            else {
                messages.push('OpenCode plugin: already installed');
            }
            break;
    }
    if (cfg.mcp) {
        const label = MCP_LABELS[cfg.mcp];
        messages.push((0, settings_inject_1.injectAgentMcp)(projectDir, cfg.mcp)
            ? `${label.name} -> ${label.file}`
            : `${label.name}: already registered`);
    }
    return messages;
}
function uninstallPlatform(platform, projectDir) {
    const messages = [];
    if (platform === 'cursor') {
        if ((0, settings_inject_1.removeCursorRule)(projectDir)) {
            messages.push('Cursor rule: removed');
        }
        else {
            messages.push('Cursor rule: not found');
        }
        messages.push((0, settings_inject_1.removeAgentMcp)(projectDir, 'cursor')
            ? 'Cursor MCP server: removed'
            : 'Cursor MCP server: not found');
        return messages;
    }
    if (platform === 'kiro') {
        const cfg = platforms_1.PLATFORMS.kiro;
        if (cfg.skillFile) {
            const homeDir = os.homedir();
            const dst = path.join(homeDir, cfg.skillDst);
            try {
                fs.unlinkSync(dst);
                messages.push(`Skill file removed: ${dst}`);
            }
            catch {
                messages.push('Skill file: not found');
            }
        }
        messages.push(...removeLegacySkillFile('kiro', cfg));
        if ((0, settings_inject_1.removeKiroSteering)(projectDir)) {
            messages.push('Kiro steering: removed');
        }
        else {
            messages.push('Kiro steering: not found');
        }
        return messages;
    }
    const cfg = platforms_1.PLATFORMS[platform];
    if (!cfg) {
        messages.push(`Unknown platform: ${platform}`);
        return messages;
    }
    if (cfg.skillFile) {
        const homeDir = os.homedir();
        let dst = path.join(homeDir, cfg.skillDst);
        if (platform === 'claude') {
            const configDir = process.env.CLAUDE_CONFIG_DIR;
            if (configDir)
                dst = path.join(configDir, 'skills', 'astria', 'SKILL.md');
        }
        try {
            fs.unlinkSync(dst);
            messages.push(`Skill file removed: ${dst}`);
        }
        catch {
            messages.push('Skill file: not found');
        }
    }
    messages.push(...removeLegacySkillFile(platform, cfg));
    if (cfg.claudeMd) {
        const claudeMdPath = path.join(os.homedir(), '.claude', 'CLAUDE.md');
        (0, markdown_inject_1.removeSection)(claudeMdPath);
        messages.push('User CLAUDE.md: astria section removed');
    }
    if (cfg.claudeMd) {
        (0, markdown_inject_1.removeSection)(path.join(projectDir, 'CLAUDE.md'));
        messages.push('Project CLAUDE.md: astria section removed');
    }
    if (cfg.agentsMd) {
        (0, markdown_inject_1.removeSection)(path.join(projectDir, 'AGENTS.md'));
        messages.push('Project AGENTS.md: astria section removed');
    }
    if (cfg.geminiMd) {
        (0, markdown_inject_1.removeSection)(path.join(projectDir, 'GEMINI.md'));
        messages.push('Project GEMINI.md: astria section removed');
    }
    switch (cfg.settingsHook) {
        case 'claude':
            (0, settings_inject_1.removeClaudeHook)(projectDir);
            messages.push('Claude PreToolUse hook: removed');
            break;
        case 'codex':
            (0, settings_inject_1.removeCodexHook)(projectDir);
            messages.push('Codex PreToolUse hook: removed');
            break;
        case 'gemini':
            (0, settings_inject_1.removeGeminiHook)(projectDir);
            messages.push('Gemini BeforeTool hook: removed');
            break;
        case 'opencode':
            (0, settings_inject_1.removeOpenCodePlugin)(projectDir);
            messages.push('OpenCode plugin: removed');
            break;
    }
    if (cfg.mcp) {
        const label = MCP_LABELS[cfg.mcp];
        messages.push((0, settings_inject_1.removeAgentMcp)(projectDir, cfg.mcp)
            ? `${label.name}: removed`
            : `${label.name}: not found`);
    }
    return messages;
}
//# sourceMappingURL=index.js.map