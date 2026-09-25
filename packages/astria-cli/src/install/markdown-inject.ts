import * as fs from 'fs';
import * as path from 'path';

export type SectionResult = 'added' | 'updated' | 'unchanged';

// Managed sections carry this marker so later installs can refresh them
// wholesale. Legacy generated sections (pre-marker) are recognized by
// fingerprint; anything else containing a graphify heading is treated as
// user-owned and left untouched.
export const SECTION_MARKER = '<!-- nodesify-graphify:managed -->';

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

function headingLevel(header: string): number {
  return (header.match(/^#+/) || ['#'])[0].length;
}

// Locate a graphify section headed by `header` (matched at line start) and
// bounded by the next heading at the same or higher level, or EOF.
function findSection(existing: string, header: string): { start: number; end: number } | null {
  let start = existing.indexOf('\n' + header);
  if (start === -1) {
    if (!existing.startsWith(header)) return null;
    start = 0;
  }
  const level = headingLevel(header);
  const boundary = new RegExp(`\\n#{1,${level}} `);
  const match = existing.slice(start + 1).match(boundary);
  const end = match?.index !== undefined ? start + 1 + match.index : existing.length;
  return { start, end };
}

function stripMarker(sectionText: string): string {
  return sectionText
    .split('\n')
    .filter((line) => line.trim() !== SECTION_MARKER)
    .join('\n')
    .trim();
}

function withMarker(content: string): string {
  return content.trimEnd() + '\n' + SECTION_MARKER;
}

export function injectSection(filePath: string, content: string): SectionResult {
  const dir = path.dirname(filePath);
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }

  const header = content.trimStart().split('\n')[0].trim();
  const existing = fs.existsSync(filePath) ? fs.readFileSync(filePath, 'utf-8') : '';

  for (const h of GRAPHIFY_HEADERS) {
    const bounds = findSection(existing, h);
    if (!bounds) continue;
    const sectionText = existing.slice(bounds.start, bounds.end);
    if (stripMarker(sectionText) === content.trim()) return 'unchanged';
    const managed =
      sectionText.includes(SECTION_MARKER) ||
      LEGACY_SECTION_SNIPPETS.some((snippet) => sectionText.includes(snippet));
    if (!managed) return 'unchanged';
    fs.writeFileSync(
      filePath,
      existing.slice(0, bounds.start) + withMarker(content) + existing.slice(bounds.end),
      'utf-8'
    );
    return 'updated';
  }

  fs.writeFileSync(filePath, existing.replace(/\n*$/, '\n\n') + withMarker(content) + '\n', 'utf-8');
  return 'added';
}

export function removeSection(filePath: string): boolean {
  if (!fs.existsSync(filePath)) return false;

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
  if (!changed) return false;

  if (content.trim().length === 0) {
    fs.unlinkSync(filePath);
  } else {
    fs.writeFileSync(filePath, content, 'utf-8');
  }
  return true;
}

export const PROJECT_MD_SECTION = `## graphify

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

export const SKILL_REGISTRATION = `
# graphify
- **graphify** (\`~/.claude/skills/graphify/SKILL.md\`) - any input to knowledge graph. Trigger: \`/graphify\`
When the user types \`/graphify\`, invoke the Skill tool with \`skill: "graphify"\` before doing anything else.
`;
