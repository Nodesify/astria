import * as fs from 'fs';
import * as path from 'path';

export type SectionResult = 'added' | 'updated' | 'unchanged';

// Managed sections carry this marker so later installs can refresh them
// wholesale. Sections are recognized two ways: by this marker, or by a
// legacy fingerprint. Pre-1.0 installs wrote `## graphify` sections under
// the old `nodesify-graphify:managed` marker — install upgrades them in
// place and uninstall removes them. Anything else containing a heading is
// treated as user-owned and left untouched.
export const SECTION_MARKER = '<!-- astria:managed -->';

// Section written by this version of the CLI.
const HEADERS = ['## astria', '# astria'];
// Sections written by pre-1.0 nodesify-graphify installs.
const LEGACY_HEADERS = ['## graphify', '# graphify'];
const LEGACY_SECTION_MARKER = '<!-- nodesify-graphify:managed -->';

const LEGACY_SECTION_SNIPPETS = [
  // Current managed sections carry the old marker only; these fingerprints
  // catch unmarked generated sections from earlier eras.
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

// Locate a section headed by `header` (matched at line start) and bounded
// by the next heading at the same or higher level, or EOF.
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
    .filter((line) => {
      const t = line.trim();
      return t !== SECTION_MARKER && t !== LEGACY_SECTION_MARKER;
    })
    .join('\n')
    .trim();
}

function isManaged(sectionText: string): boolean {
  return (
    sectionText.includes(SECTION_MARKER) ||
    sectionText.includes(LEGACY_SECTION_MARKER) ||
    LEGACY_SECTION_SNIPPETS.some((snippet) => sectionText.includes(snippet))
  );
}

function withMarker(content: string): string {
  return content.trimEnd() + '\n' + SECTION_MARKER;
}

export function injectSection(filePath: string, content: string): SectionResult {
  const dir = path.dirname(filePath);
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }

  const existing = fs.existsSync(filePath) ? fs.readFileSync(filePath, 'utf-8') : '';

  // Refresh an existing astria section in place.
  for (const h of HEADERS) {
    const bounds = findSection(existing, h);
    if (!bounds) continue;
    const sectionText = existing.slice(bounds.start, bounds.end);
    if (stripMarker(sectionText) === content.trim()) return 'unchanged';
    if (!isManaged(sectionText)) return 'unchanged';
    fs.writeFileSync(
      filePath,
      existing.slice(0, bounds.start) + withMarker(content) + existing.slice(bounds.end),
      'utf-8'
    );
    return 'updated';
  }

  // Upgrade a managed pre-1.0 graphify section to the astria wording. A
  // user-customized graphify section is left untouched (treated as owned).
  for (const h of LEGACY_HEADERS) {
    const bounds = findSection(existing, h);
    if (!bounds) continue;
    const sectionText = existing.slice(bounds.start, bounds.end);
    if (!isManaged(sectionText)) return 'unchanged';
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
  for (const header of [...HEADERS, ...LEGACY_HEADERS]) {
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

export const PROJECT_MD_SECTION = `## astria

This project has an astria knowledge graph at .astria/.
Access it through whichever path your agent has:
- MCP (when an astria MCP server is connected): repo_map, query_graph, explain,
  get_neighbors, shortest_path, affected.
- CLI (works everywhere): astria map, query, explain, path, affected.

Always-on behaviors:
1. Prefer the graph over repeated text searches for architecture questions, feature
   location, cross-file logic flow, and change impact; orient with repo_map (or map,
   or .astria/graph_report.md), and run affected <node> before changing a shared symbol.
2. Before running grep/ripgrep to locate code, try astria query first --
   it answers with file:line provenance in one call against the already-built graph.
3. After modifying code, run astria update . (AST-only, no API cost) so the
   graph stays fresh; queries then report accurate staleness metadata.`;

export const SKILL_REGISTRATION = `
# astria
- **astria** (\`~/.claude/skills/astria/SKILL.md\`) - any input to knowledge graph. Trigger: \`/astria\`
When the user types \`/astria\`, invoke the Skill tool with \`skill: "astria"\` before doing anything else.
`;
