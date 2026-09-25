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
exports.SKILL_REGISTRATION = exports.PROJECT_MD_SECTION = void 0;
exports.injectSection = injectSection;
exports.removeSection = removeSection;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const SECTION_HEADER = '## graphify';
function injectSection(filePath, content) {
    const dir = path.dirname(filePath);
    if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
    }
    let existing = '';
    if (fs.existsSync(filePath)) {
        existing = fs.readFileSync(filePath, 'utf-8');
    }
    if (existing.includes(SECTION_HEADER)) {
        return false;
    }
    const section = '\n' + content + '\n';
    fs.writeFileSync(filePath, existing + section, 'utf-8');
    return true;
}
function removeSection(filePath) {
    if (!fs.existsSync(filePath)) {
        return false;
    }
    let content = fs.readFileSync(filePath, 'utf-8');
    const regex = new RegExp('\\n*' + SECTION_HEADER.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '\\n.*?(?=\\n## |$)', 'gs');
    const updated = content.replace(regex, '');
    if (updated.trim().length === 0) {
        fs.unlinkSync(filePath);
    }
    else {
        fs.writeFileSync(filePath, updated, 'utf-8');
    }
    return updated !== content;
}
exports.PROJECT_MD_SECTION = `## graphify

This project has an optional nodesify-graphify knowledge graph at .graphify/.
The graph is maintained automatically after edits when the platform supports PostToolUse hooks.
Use /graphify when you want graph-backed architecture lookup, mapping, or export; ordinary file search remains supported.

Always-on behaviors:
1. Before running grep/ripgrep to locate code, try nodesify-graphify query first --
   it answers with file:line provenance in one call against the already-built graph.
2. After modifying code, run nodesify-graphify update . (AST-only, no API cost) so the
   graph stays fresh; queries then report accurate staleness metadata.`;
exports.SKILL_REGISTRATION = `
# graphify
- **graphify** (\`~/.claude/skills/graphify/SKILL.md\`) - any input to knowledge graph. Trigger: \`/graphify\`
When the user types \`/graphify\`, invoke the Skill tool with \`skill: "graphify"\` before doing anything else.
`;
//# sourceMappingURL=markdown-inject.js.map