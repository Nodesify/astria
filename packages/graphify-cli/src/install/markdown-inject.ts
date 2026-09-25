import * as fs from 'fs';
import * as path from 'path';

const SECTION_HEADER = '## graphify';

export function injectSection(filePath: string, content: string): boolean {
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

export function removeSection(filePath: string): boolean {
  if (!fs.existsSync(filePath)) {
    return false;
  }

  let content = fs.readFileSync(filePath, 'utf-8');
  const regex = new RegExp(
    '\\n*' + SECTION_HEADER.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '\\n.*?(?=\\n## |$)',
    'gs'
  );
  const updated = content.replace(regex, '');

  if (updated.trim().length === 0) {
    fs.unlinkSync(filePath);
  } else {
    fs.writeFileSync(filePath, updated, 'utf-8');
  }
  return updated !== content;
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
