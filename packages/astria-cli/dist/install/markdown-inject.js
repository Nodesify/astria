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
exports.SKILL_REGISTRATION = exports.PROJECT_MD_SECTION = exports.SECTION_MARKER = void 0;
exports.injectSection = injectSection;
exports.removeSection = removeSection;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
// Managed sections carry this marker so later installs can refresh them
// wholesale. Legacy generated sections (pre-marker) are recognized by
// fingerprint; anything else containing a graphify heading is treated as
// user-owned and left untouched.
exports.SECTION_MARKER = '<!-- nodesify-graphify:managed -->';
const GRAPHIFY_HEADERS = ['## graphify', '# graphify'];
const LEGACY_SECTION_SNIPPETS = [
    // "optional ... opt-in" wording (0.7.x-era)
    'optional nodesify-graphify knowledge graph',
    'maintained automatically after edits when the platform supports PostToolUse hooks',
    // "MUST read" rules wording (early releases)
    'MUST read .graphify/graph_report.md before searching files',
    // "CRITICAL RULES / FORBIDDEN" wording
    '**FORBIDDEN** from using native search tools',
];
function headingLevel(header) {
    return (header.match(/^#+/) || ['#'])[0].length;
}
// Locate a graphify section headed by `header` (matched at line start) and
// bounded by the next heading at the same or higher level, or EOF.
function findSection(existing, header) {
    let start = existing.indexOf('\n' + header);
    if (start === -1) {
        if (!existing.startsWith(header))
            return null;
        start = 0;
    }
    const level = headingLevel(header);
    const boundary = new RegExp(`\\n#{1,${level}} `);
    const match = existing.slice(start + 1).match(boundary);
    const end = match?.index !== undefined ? start + 1 + match.index : existing.length;
    return { start, end };
}
function stripMarker(sectionText) {
    return sectionText
        .split('\n')
        .filter((line) => line.trim() !== exports.SECTION_MARKER)
        .join('\n')
        .trim();
}
function withMarker(content) {
    return content.trimEnd() + '\n' + exports.SECTION_MARKER;
}
function injectSection(filePath, content) {
    const dir = path.dirname(filePath);
    if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
    }
    const header = content.trimStart().split('\n')[0].trim();
    const existing = fs.existsSync(filePath) ? fs.readFileSync(filePath, 'utf-8') : '';
    for (const h of GRAPHIFY_HEADERS) {
        const bounds = findSection(existing, h);
        if (!bounds)
            continue;
        const sectionText = existing.slice(bounds.start, bounds.end);
        if (stripMarker(sectionText) === content.trim())
            return 'unchanged';
        const managed = sectionText.includes(exports.SECTION_MARKER) ||
            LEGACY_SECTION_SNIPPETS.some((snippet) => sectionText.includes(snippet));
        if (!managed)
            return 'unchanged';
        fs.writeFileSync(filePath, existing.slice(0, bounds.start) + withMarker(content) + existing.slice(bounds.end), 'utf-8');
        return 'updated';
    }
    fs.writeFileSync(filePath, existing.replace(/\n*$/, '\n\n') + withMarker(content) + '\n', 'utf-8');
    return 'added';
}
function removeSection(filePath) {
    if (!fs.existsSync(filePath))
        return false;
    let content = fs.readFileSync(filePath, 'utf-8');
    let changed = false;
    for (const header of GRAPHIFY_HEADERS) {
        let bounds = findSection(content, header);
        while (bounds) {
            content = content.slice(0, bounds.start) + content.slice(bounds.end);
            changed = true;
            bounds = findSection(content, header);
        }
    }
    if (!changed)
        return false;
    if (content.trim().length === 0) {
        fs.unlinkSync(filePath);
    }
    else {
        fs.writeFileSync(filePath, content, 'utf-8');
    }
    return true;
}
exports.PROJECT_MD_SECTION = `## graphify

This project has a nodesify-graphify knowledge graph at .graphify/.
Access it through whichever path your agent has:
- MCP (when a graphify MCP server is connected): repo_map, query_graph, explain,
  get_neighbors, shortest_path, affected.
- CLI (works everywhere): nodesify-graphify map, query, explain, path, affected.

Always-on behaviors:
1. Prefer the graph over repeated text searches for architecture questions, feature
   location, cross-file logic flow, and change impact; orient with repo_map (or map,
   or .graphify/graph_report.md), and run affected <node> before changing a shared symbol.
2. Before running grep/ripgrep to locate code, try nodesify-graphify query first --
   it answers with file:line provenance in one call against the already-built graph.
3. After modifying code, run nodesify-graphify update . (AST-only, no API cost) so the
   graph stays fresh; queries then report accurate staleness metadata.`;
exports.SKILL_REGISTRATION = `
# graphify
- **graphify** (\`~/.claude/skills/graphify/SKILL.md\`) - any input to knowledge graph. Trigger: \`/graphify\`
When the user types \`/graphify\`, invoke the Skill tool with \`skill: "graphify"\` before doing anything else.
`;
//# sourceMappingURL=markdown-inject.js.map