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
// ---- Claude Code (.claude/settings.json) ----
const CLAUDE_POST_UPDATE_HOOK = {
    matcher: 'Edit|Write',
    hooks: [{
            type: 'command',
            // Detached and debounced so editing never waits for a graph rebuild.
            command: `node -e "const fs=require('fs'),cp=require('child_process'),path=require('path');try{const root=cp.execSync('git rev-parse --show-toplevel',{encoding:'utf8'}).trim();const stamp=path.join(root,'.graphify','.posttool-update');if(fs.existsSync(stamp)&&Date.now()-fs.statSync(stamp).mtimeMs<120000)process.exit(0);fs.mkdirSync(path.dirname(stamp),{recursive:true});fs.writeFileSync(stamp,String(Date.now()));const cli=fs.existsSync(path.join(root,'packages','graphify-cli','dist','index.js'))?'node packages/graphify-cli/dist/index.js update .':'npx --no-install nodesify-graphify update .';cp.spawn(cli,{cwd:root,shell:true,detached:true,stdio:'ignore'}).unref()}catch{}"`,
        }],
};
function injectClaudeHook(projectDir) {
    const settingsPath = path.join(projectDir, '.claude', 'settings.json');
    const data = readJson(settingsPath);
    if (!data.hooks)
        data.hooks = {};
    if (!data.hooks.PreToolUse)
        data.hooks.PreToolUse = [];
    const existing = data.hooks.PreToolUse;
    const hadLegacy = existing.some((h) => JSON.stringify(h.hooks).includes('graphify'));
    // Remove legacy graphify PreToolUse nags when upgrading.
    data.hooks.PreToolUse = existing.filter((h) => !JSON.stringify(h.hooks).includes('graphify'));
    if (data.hooks.PreToolUse.length === 0)
        delete data.hooks.PreToolUse;
    const post = data.hooks.PostToolUse || [];
    const hadPost = post.some((h) => JSON.stringify(h.hooks).includes('graphify'));
    if (!hadPost)
        post.push(CLAUDE_POST_UPDATE_HOOK);
    data.hooks.PostToolUse = post;
    writeJson(settingsPath, data);
    return !hadPost || hadLegacy;
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
            data.hooks[event] = data.hooks[event].filter((h) => !JSON.stringify(h.hooks).includes('graphify'));
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
function injectCodexHook(projectDir) {
    const hooksPath = path.join(projectDir, '.codex', 'hooks.json');
    const data = readJson(hooksPath);
    if (!data.hooks)
        data.hooks = {};
    if (!data.hooks.PreToolUse)
        data.hooks.PreToolUse = [];
    const existing = data.hooks.PreToolUse;
    const alreadyExists = existing.some((h) => JSON.stringify(h.hooks || []).includes('graphify'));
    if (alreadyExists)
        return false;
    existing.push({
        matcher: 'Bash',
        hooks: [{
                type: 'command',
                command: `node -e "const fs=require('fs');const p='.graphify/graph.json';if(!fs.existsSync(p)){process.exit(0)}const msg='nodesify-graphify: Knowledge graph available. Use nodesify-graphify query for architecture questions. Read .graphify/graph_report.md first.';process.stdout.write(JSON.stringify({hookSpecificOutput:{hookEventName:'PreToolUse',additionalContext:msg}}))"`,
            }],
    });
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
    data.hooks.PreToolUse = data.hooks.PreToolUse.filter((h) => !JSON.stringify(h.hooks || []).includes('graphify'));
    writeJson(hooksPath, data);
    return data.hooks.PreToolUse.length !== before;
}
// ---- Gemini (.gemini/settings.json) ----
function injectGeminiHook(projectDir) {
    const settingsPath = path.join(projectDir, '.gemini', 'settings.json');
    const data = readJson(settingsPath);
    if (!data.hooks)
        data.hooks = {};
    if (!data.hooks.BeforeTool)
        data.hooks.BeforeTool = [];
    const existing = data.hooks.BeforeTool;
    const alreadyExists = existing.some((h) => h.matcher === 'read_file|list_directory' && JSON.stringify(h.hooks || []).includes('graphify'));
    if (alreadyExists)
        return false;
    existing.push({
        matcher: 'read_file|list_directory',
        hooks: [{
                type: 'command',
                command: `node -e "const fs=require('fs');const p='.graphify/graph.json';var r={decision:'allow'};if(fs.existsSync(p)){r.additionalContext='nodesify-graphify: Knowledge graph available. Use nodesify-graphify query for architecture questions. Read .graphify/graph_report.md first.'}process.stdout.write(JSON.stringify(r))"`,
            }],
    });
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
    data.hooks.BeforeTool = data.hooks.BeforeTool.filter((h) => !JSON.stringify(h.hooks || []).includes('graphify'));
    writeJson(settingsPath, data);
    return data.hooks.BeforeTool.length !== before;
}
// ---- OpenCode (.opencode/) ----
const OPENCODE_PLUGIN_JS = `// nodesify-graphify OpenCode plugin
import { existsSync } from "fs";
import { join } from "path";

export const GraphifyPlugin = async ({ directory }) => {
  const reminded = new Set();
  return {
    "tool.execute.before": async (input, output) => {
      if (reminded.has(input.tool)) return;
      if (!["view", "grep", "glob", "ls", "bash"].includes(input.tool)) return;
      if (!existsSync(join(directory, ".graphify", "graph.json"))) return;
      if (input.tool === "bash") {
        output.args.command =
          'echo "[nodesify-graphify] Knowledge graph available. MUST read .graphify/graph_report.md before searching raw files. Use nodesify-graphify query instead of grep for architecture questions." && ' +
          output.args.command;
      } else {
        output.error = new Error(
          "[nodesify-graphify] Knowledge graph available. MUST read .graphify/graph_report.md before searching raw files. Use nodesify-graphify query instead of grep for architecture questions."
        );
      }
      reminded.add(input.tool);
    },
  };
};
`;
function injectOpenCodePlugin(projectDir) {
    const pluginDir = path.join(projectDir, '.opencode', 'plugins');
    const pluginPath = path.join(pluginDir, 'graphify.js');
    if (fs.existsSync(pluginPath))
        return false;
    if (!fs.existsSync(pluginDir)) {
        fs.mkdirSync(pluginDir, { recursive: true });
    }
    fs.writeFileSync(pluginPath, OPENCODE_PLUGIN_JS, 'utf-8');
    const configPath = path.join(projectDir, '.opencode', 'opencode.json');
    const config = readJson(configPath);
    if (!config.plugins)
        config.plugins = [];
    if (!config.plugins.includes('./plugins/graphify.js')) {
        config.plugins.push('./plugins/graphify.js');
    }
    writeJson(configPath, config);
    return true;
}
function removeOpenCodePlugin(projectDir) {
    const pluginPath = path.join(projectDir, '.opencode', 'plugins', 'graphify.js');
    if (!fs.existsSync(pluginPath))
        return false;
    fs.unlinkSync(pluginPath);
    const configPath = path.join(projectDir, '.opencode', 'opencode.json');
    const config = readJson(configPath);
    if (config.plugins) {
        config.plugins = config.plugins.filter((p) => p !== './plugins/graphify.js');
        writeJson(configPath, config);
    }
    return true;
}
// ---- Cursor (.cursor/rules/graphify.mdc) ----
const CURSOR_RULE = `---
description: nodesify-graphify knowledge graph context
alwaysApply: true
---

This project has a nodesify-graphify knowledge graph at .graphify/.

Rules:
- MUST read .graphify/graph_report.md before searching files for architecture or codebase questions
- MUST use \`nodesify-graphify query "<question>"\`, \`nodesify-graphify path "<A>" "<B>"\`, or \`nodesify-graphify explain "<concept>"\` for cross-module questions — do NOT grep/read files directly for these
- After modifying code files, run \`nodesify-graphify update .\` to keep the graph current
`;
function injectCursorRule(projectDir) {
    const ruleDir = path.join(projectDir, '.cursor', 'rules');
    const rulePath = path.join(ruleDir, 'graphify.mdc');
    if (fs.existsSync(rulePath))
        return false;
    if (!fs.existsSync(ruleDir)) {
        fs.mkdirSync(ruleDir, { recursive: true });
    }
    fs.writeFileSync(rulePath, CURSOR_RULE, 'utf-8');
    return true;
}
function removeCursorRule(projectDir) {
    const rulePath = path.join(projectDir, '.cursor', 'rules', 'graphify.mdc');
    if (!fs.existsSync(rulePath))
        return false;
    fs.unlinkSync(rulePath);
    return true;
}
const GRAPHIFY_MCP_SERVER = { type: 'stdio', command: 'nodesify-graphify', args: ['mcp'] };
const MCP_TARGETS = {
    zcode: { configPath: path.join('.zcode', 'config.json'), serverPath: ['mcp', 'servers', 'graphify'] },
    claude: { configPath: '.mcp.json', serverPath: ['mcpServers', 'graphify'] },
    cursor: { configPath: path.join('.cursor', 'mcp.json'), serverPath: ['mcpServers', 'graphify'] },
    gemini: { configPath: path.join('.gemini', 'settings.json'), serverPath: ['mcpServers', 'graphify'] },
};
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
    if (node[name])
        return false;
    node[name] = { ...GRAPHIFY_MCP_SERVER };
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
    if (!node[name])
        return false;
    delete node[name];
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
// ---- Kiro (.kiro/steering/graphify.md) ----
const KIRO_STEERING = `---
inclusion: always
---

nodesify-graphify: A knowledge graph of this project lives in \`.graphify/\`.

Rules:
- MUST read \`.graphify/graph_report.md\` before searching files for architecture or codebase questions
- MUST use \`nodesify-graphify query\`, \`nodesify-graphify path\`, or \`nodesify-graphify explain\` for cross-module questions — do NOT grep/read files directly
- After modifying code files, run \`nodesify-graphify update .\` to keep the graph current
`;
function injectKiroSteering(projectDir) {
    const steerDir = path.join(projectDir, '.kiro', 'steering');
    const steerPath = path.join(steerDir, 'graphify.md');
    if (fs.existsSync(steerPath))
        return false;
    if (!fs.existsSync(steerDir)) {
        fs.mkdirSync(steerDir, { recursive: true });
    }
    fs.writeFileSync(steerPath, KIRO_STEERING, 'utf-8');
    return true;
}
function removeKiroSteering(projectDir) {
    const steerPath = path.join(projectDir, '.kiro', 'steering', 'graphify.md');
    if (!fs.existsSync(steerPath))
        return false;
    fs.unlinkSync(steerPath);
    return true;
}
//# sourceMappingURL=settings-inject.js.map