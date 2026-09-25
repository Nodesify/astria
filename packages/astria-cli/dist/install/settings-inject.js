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
exports.injectClaudeHook = injectClaudeHook;
exports.removeClaudeHook = removeClaudeHook;
exports.injectCodexHook = injectCodexHook;
exports.removeCodexHook = removeCodexHook;
exports.injectGeminiHook = injectGeminiHook;
exports.removeGeminiHook = removeGeminiHook;
exports.injectOpenCodePlugin = injectOpenCodePlugin;
exports.removeOpenCodePlugin = removeOpenCodePlugin;
exports.injectCursorRule = injectCursorRule;
exports.removeCursorRule = removeCursorRule;
exports.injectAgentMcp = injectAgentMcp;
exports.removeAgentMcp = removeAgentMcp;
exports.injectZcodeMcp = injectZcodeMcp;
exports.removeZcodeMcp = removeZcodeMcp;
exports.injectKiroSteering = injectKiroSteering;
exports.removeKiroSteering = removeKiroSteering;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
function readJson(filePath) {
    if (!fs.existsSync(filePath))
        return {};
    try {
        return JSON.parse(fs.readFileSync(filePath, 'utf-8'));
    }
    catch {
        return {};
    }
}
function writeJson(filePath, data) {
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
const isCurrent = (s) => s.includes('astria');
const isLegacy = (s) => !s.includes('astria') && s.includes('graphify');
const isAnyEra = (s) => s.includes('astria') || s.includes('graphify');
// ---- Claude Code (.claude/settings.json) ----
const CLAUDE_POST_UPDATE_HOOK = {
    matcher: 'Edit|Write',
    hooks: [{
            type: 'command',
            // Detached and debounced so editing never waits for a graph rebuild.
            command: `node -e "const fs=require('fs'),cp=require('child_process'),path=require('path');try{const root=cp.execSync('git rev-parse --show-toplevel',{encoding:'utf8'}).trim();const stamp=path.join(root,'.astria','.posttool-update');if(fs.existsSync(stamp)&&Date.now()-fs.statSync(stamp).mtimeMs<120000)process.exit(0);fs.mkdirSync(path.dirname(stamp),{recursive:true});fs.writeFileSync(stamp,String(Date.now()));const cli=fs.existsSync(path.join(root,'packages','astria-cli','dist','index.js'))?'node packages/astria-cli/dist/index.js update .':'npx --no-install astria update .';cp.spawn(cli,{cwd:root,shell:true,detached:true,stdio:'ignore'}).unref()}catch{}"`,
        }],
};
function injectClaudeHook(projectDir) {
    const settingsPath = path.join(projectDir, '.claude', 'settings.json');
    const data = readJson(settingsPath);
    if (!data.hooks)
        data.hooks = {};
    const pre = (data.hooks.PreToolUse || []);
    const hadLegacyPre = pre.some((h) => isLegacy(JSON.stringify(h.hooks)));
    // Remove pre-1.0 graphify nags when upgrading.
    data.hooks.PreToolUse = pre.filter((h) => !isLegacy(JSON.stringify(h.hooks)));
    if (data.hooks.PreToolUse.length === 0)
        delete data.hooks.PreToolUse;
    const post = (data.hooks.PostToolUse || []);
    const hadLegacyPost = post.some((h) => isLegacy(JSON.stringify(h.hooks)));
    const hadPost = post.some((h) => isCurrent(JSON.stringify(h.hooks)));
    const upgraded = post.filter((h) => !isLegacy(JSON.stringify(h.hooks)));
    if (!hadPost)
        upgraded.push(CLAUDE_POST_UPDATE_HOOK);
    data.hooks.PostToolUse = upgraded;
    if (Object.keys(data.hooks).length === 0)
        delete data.hooks;
    writeJson(settingsPath, data);
    return !hadPost || hadLegacyPre || hadLegacyPost;
}
function removeClaudeHook(projectDir) {
    const settingsPath = path.join(projectDir, '.claude', 'settings.json');
    if (!fs.existsSync(settingsPath))
        return false;
    const data = readJson(settingsPath);
    if (!data.hooks)
        return false;
    const before = JSON.stringify(data.hooks);
    for (const event of ['PreToolUse', 'PostToolUse']) {
        if (data.hooks[event]) {
            data.hooks[event] = data.hooks[event].filter((h) => !isAnyEra(JSON.stringify(h.hooks)));
            if (data.hooks[event].length === 0)
                delete data.hooks[event];
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
function injectCodexHook(projectDir) {
    const hooksPath = path.join(projectDir, '.codex', 'hooks.json');
    const data = readJson(hooksPath);
    if (!data.hooks)
        data.hooks = {};
    const existing = (data.hooks.PreToolUse || []);
    const hadLegacy = existing.some((h) => isLegacy(JSON.stringify(h.hooks || [])));
    const current = existing.filter((h) => !isLegacy(JSON.stringify(h.hooks || [])));
    const alreadyExists = current.some((h) => isCurrent(JSON.stringify(h.hooks || [])));
    if (alreadyExists && !hadLegacy)
        return false;
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
function removeCodexHook(projectDir) {
    const hooksPath = path.join(projectDir, '.codex', 'hooks.json');
    if (!fs.existsSync(hooksPath))
        return false;
    const data = readJson(hooksPath);
    if (!data.hooks?.PreToolUse)
        return false;
    const before = data.hooks.PreToolUse.length;
    data.hooks.PreToolUse = data.hooks.PreToolUse.filter((h) => !isAnyEra(JSON.stringify(h.hooks || [])));
    writeJson(hooksPath, data);
    return data.hooks.PreToolUse.length !== before;
}
// ---- Gemini (.gemini/settings.json) ----
const GEMINI_HOOK_COMMAND = `node -e "const fs=require('fs');const p='.astria/graph.json';var r={decision:'allow'};if(fs.existsSync(p)){r.additionalContext='astria: Knowledge graph available. Use astria query for architecture questions. Read .astria/graph_report.md first.'}process.stdout.write(JSON.stringify(r))"`;
function injectGeminiHook(projectDir) {
    const settingsPath = path.join(projectDir, '.gemini', 'settings.json');
    const data = readJson(settingsPath);
    if (!data.hooks)
        data.hooks = {};
    const existing = (data.hooks.BeforeTool || []);
    const hadLegacy = existing.some((h) => h.matcher === 'read_file|list_directory' && isLegacy(JSON.stringify(h.hooks || [])));
    const current = existing.filter((h) => !(h.matcher === 'read_file|list_directory' && isLegacy(JSON.stringify(h.hooks || []))));
    const alreadyExists = current.some((h) => h.matcher === 'read_file|list_directory' && isCurrent(JSON.stringify(h.hooks || [])));
    if (alreadyExists && !hadLegacy)
        return false;
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
function removeGeminiHook(projectDir) {
    const settingsPath = path.join(projectDir, '.gemini', 'settings.json');
    if (!fs.existsSync(settingsPath))
        return false;
    const data = readJson(settingsPath);
    if (!data.hooks?.BeforeTool)
        return false;
    const before = data.hooks.BeforeTool.length;
    data.hooks.BeforeTool = data.hooks.BeforeTool.filter((h) => !isAnyEra(JSON.stringify(h.hooks || [])));
    writeJson(settingsPath, data);
    return data.hooks.BeforeTool.length !== before;
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
function injectOpenCodePlugin(projectDir) {
    const pluginDir = path.join(projectDir, '.opencode', 'plugins');
    const pluginPath = path.join(pluginDir, 'astria.js');
    // Pre-1.0 installs registered plugins/graphify.js — drop it on upgrade.
    const legacyPath = path.join(pluginDir, 'graphify.js');
    if (fs.existsSync(legacyPath))
        fs.unlinkSync(legacyPath);
    if (!fs.existsSync(pluginDir)) {
        fs.mkdirSync(pluginDir, { recursive: true });
    }
    fs.writeFileSync(pluginPath, OPENCODE_PLUGIN_JS, 'utf-8');
    const configPath = path.join(projectDir, '.opencode', 'opencode.json');
    const config = readJson(configPath);
    if (!config.plugins)
        config.plugins = [];
    if (!config.plugins.includes('./plugins/astria.js')) {
        config.plugins.push('./plugins/astria.js');
    }
    config.plugins = config.plugins.filter((p) => p !== './plugins/graphify.js');
    writeJson(configPath, config);
    return true;
}
function removeOpenCodePlugin(projectDir) {
    let changed = false;
    for (const name of ['astria.js', 'graphify.js']) {
        const pluginPath = path.join(projectDir, '.opencode', 'plugins', name);
        if (fs.existsSync(pluginPath)) {
            fs.unlinkSync(pluginPath);
            changed = true;
        }
    }
    const configPath = path.join(projectDir, '.opencode', 'opencode.json');
    const config = readJson(configPath);
    if (config.plugins) {
        const filtered = config.plugins.filter((p) => p !== './plugins/astria.js' && p !== './plugins/graphify.js');
        if (filtered.length !== config.plugins.length) {
            config.plugins = filtered;
            writeJson(configPath, config);
            changed = true;
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
function injectCursorRule(projectDir) {
    const ruleDir = path.join(projectDir, '.cursor', 'rules');
    const rulePath = path.join(ruleDir, 'astria.mdc');
    // Pre-1.0 installs wrote graphify.mdc — drop it on upgrade.
    const legacyPath = path.join(ruleDir, 'graphify.mdc');
    if (fs.existsSync(legacyPath))
        fs.unlinkSync(legacyPath);
    if (!fs.existsSync(ruleDir)) {
        fs.mkdirSync(ruleDir, { recursive: true });
    }
    fs.writeFileSync(rulePath, CURSOR_RULE, 'utf-8');
    return true;
}
function removeCursorRule(projectDir) {
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
const ASTRIA_MCP_SERVER = { type: 'stdio', command: 'astria', args: ['mcp'] };
// Server registration key per flavor. The legacy "graphify" key is matched
// for upgrade (inject) and cleanup (remove).
const MCP_TARGETS = {
    zcode: { configPath: path.join('.zcode', 'config.json'), serverPath: ['mcp', 'servers', 'astria'] },
    claude: { configPath: '.mcp.json', serverPath: ['mcpServers', 'astria'] },
    cursor: { configPath: path.join('.cursor', 'mcp.json'), serverPath: ['mcpServers', 'astria'] },
    gemini: { configPath: path.join('.gemini', 'settings.json'), serverPath: ['mcpServers', 'astria'] },
};
const LEGACY_MCP_SERVER_NAME = 'graphify';
function injectAgentMcp(projectDir, flavor) {
    const target = MCP_TARGETS[flavor];
    const configPath = path.join(projectDir, target.configPath);
    const data = readJson(configPath);
    let node = data;
    for (const key of target.serverPath.slice(0, -1)) {
        if (typeof node[key] !== 'object' || node[key] === null)
            node[key] = {};
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
    if (node[name]) {
        if (removedLegacy) {
            writeJson(configPath, data);
            return true;
        }
        return false;
    }
    node[name] = { ...ASTRIA_MCP_SERVER };
    writeJson(configPath, data);
    return true;
}
function removeAgentMcp(projectDir, flavor) {
    const target = MCP_TARGETS[flavor];
    const configPath = path.join(projectDir, target.configPath);
    if (!fs.existsSync(configPath))
        return false;
    const data = readJson(configPath);
    const parents = [];
    let node = data;
    for (const key of target.serverPath.slice(0, -1)) {
        if (typeof node[key] !== 'object' || node[key] === null)
            return false;
        parents.push([node, key]);
        node = node[key];
    }
    const name = target.serverPath[target.serverPath.length - 1];
    if (!node[name] && !node[LEGACY_MCP_SERVER_NAME])
        return false;
    delete node[name];
    delete node[LEGACY_MCP_SERVER_NAME];
    for (let i = parents.length - 1; i >= 0; i--) {
        const [parent, key] = parents[i];
        if (Object.keys(parent[key]).length === 0)
            delete parent[key];
    }
    writeJson(configPath, data);
    return true;
}
function injectZcodeMcp(projectDir) {
    return injectAgentMcp(projectDir, 'zcode');
}
function removeZcodeMcp(projectDir) {
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
function injectKiroSteering(projectDir) {
    const steerDir = path.join(projectDir, '.kiro', 'steering');
    const steerPath = path.join(steerDir, 'astria.md');
    // Pre-1.0 installs wrote graphify.md — drop it on upgrade.
    const legacyPath = path.join(steerDir, 'graphify.md');
    if (fs.existsSync(legacyPath))
        fs.unlinkSync(legacyPath);
    if (!fs.existsSync(steerDir)) {
        fs.mkdirSync(steerDir, { recursive: true });
    }
    fs.writeFileSync(steerPath, KIRO_STEERING, 'utf-8');
    return true;
}
function removeKiroSteering(projectDir) {
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
//# sourceMappingURL=settings-inject.js.map