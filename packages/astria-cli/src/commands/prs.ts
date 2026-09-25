// PR impact dashboard: maps open PRs' changed files onto the knowledge graph
// (communities touched, node blast radius) and flags merge-order risk.
// Ported from upstream graphify v8 prs.py. Requires the `gh` CLI (read-only).

import { execFileSync } from 'child_process';
import { existsSync, readFileSync } from 'fs';
import { join } from 'path';

interface GhPR {
  number: number;
  title: string;
  headRefName: string;
  baseRefName: string;
  isDraft: boolean;
}

interface PrImpact {
  number: number;
  title: string;
  draft: boolean;
  files: string[];
  nodes: Set<string>;
  communities: Map<number, number>; // community -> node count
}

const DIM = '\x1b[2m';
const BOLD = '\x1b[1m';
const YELLOW = '\x1b[33m';
const CYAN = '\x1b[36m';
const RESET = '\x1b[0m';

function ghAvailable(): boolean {
  try {
    execFileSync('gh', ['--version'], { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}

function listOpenPrs(limit: number): GhPR[] {
  const out = execFileSync(
    'gh',
    ['pr', 'list', '--limit', String(limit), '--json', 'number,title,headRefName,baseRefName,isDraft'],
    { encoding: 'utf-8', stdio: ['pipe', 'pipe', 'pipe'] },
  );
  return JSON.parse(out);
}

function prFiles(number: number): string[] {
  if (!Number.isInteger(number) || number <= 0) return [];
  try {
    const out = execFileSync(
      'gh',
      ['pr', 'view', String(number), '--json', 'files'],
      { encoding: 'utf-8', stdio: ['pipe', 'pipe', 'pipe'] },
    );
    const parsed = JSON.parse(out);
    return (parsed.files || []).map((f: { path: string }) => f.path);
  } catch {
    return [];
  }
}

function loadGraphNodes(graphRoot: string): { source_file: string; community: number | null }[] {
  const graphJson = join(graphRoot, '.graphify', 'graph.json');
  if (!existsSync(graphJson)) {
    throw new Error(`no graph found at ${graphJson} — run 'nodesify-graphify run ${graphRoot}' first`);
  }
  const graph = JSON.parse(readFileSync(graphJson, 'utf-8'));
  return (graph.nodes || []).map((n: any) => ({
    source_file: String(n.source_file || '').replace(/\\/g, '/'),
    community: n.community ?? null,
  }));
}

function computeImpact(pr: GhPR, files: string[], nodes: { source_file: string; community: number | null }[]): PrImpact {
  const normalized = files.map((f) => f.replace(/\\/g, '/'));
  const impact: PrImpact = {
    number: pr.number,
    title: pr.title,
    draft: pr.isDraft,
    files,
    nodes: new Set(),
    communities: new Map(),
  };
  for (const node of nodes) {
    if (!node.source_file) continue;
    const hit = normalized.some(
      (f) => node.source_file === f || node.source_file.endsWith('/' + f),
    );
    if (hit) {
      impact.nodes.add(node.source_file);
      if (node.community !== null) {
        impact.communities.set(node.community, (impact.communities.get(node.community) || 0) + 1);
      }
    }
  }
  return impact;
}

function printTable(impacts: PrImpact[]): void {
  console.log(`${BOLD}Open pull requests mapped onto the knowledge graph${RESET}\n`);
  const numW = String(Math.max(...impacts.map((i) => i.number), 0)).length;
  for (const pr of impacts) {
    const draft = pr.draft ? `${DIM}[draft]${RESET} ` : '';
    const top = [...pr.communities.entries()].sort((a, b) => b[1] - a[1]).slice(0, 4);
    const commStr = top.map(([c, n]) => `${CYAN}${c}${RESET}:${n}`).join(' ') || '—';
    const symbols = [...pr.communities.values()].reduce((a, b) => a + b, 0);
    console.log(
      `${DIM}#${String(pr.number).padStart(numW)}${RESET} ${draft}${pr.title}`,
    );
    console.log(
      `     ${DIM}${pr.files.length} file(s) · ${symbols} graph symbols · communities: ${commStr}${RESET}`,
    );
  }
}

function printConflicts(impacts: PrImpact[]): void {
  console.log(`\n${BOLD}Merge-order risk (shared communities)${RESET}\n`);
  let found = false;
  for (let i = 0; i < impacts.length; i++) {
    for (let j = i + 1; j < impacts.length; j++) {
      const shared = [...impacts[i].communities.keys()].filter((c) =>
        impacts[j].communities.has(c),
      );
      if (shared.length > 0) {
        found = true;
        console.log(
          `${YELLOW}#${impacts[i].number} ↔ #${impacts[j].number}${RESET} share ${shared.length} communit${shared.length === 1 ? 'y' : 'ies'}: ${shared.slice(0, 8).join(', ')}${shared.length > 8 ? ' …' : ''}`,
        );
      }
    }
  }
  if (!found) console.log('No shared communities between open PRs.');
}

export async function prsCommand(count: string, opts: { graph: string; conflicts: boolean }) {
  const limit = parseInt(count, 10) || 20;
  try {
    if (!ghAvailable()) {
      console.error('Error: the GitHub CLI (gh) is required for this command — https://cli.github.com');
      process.exitCode = 1;
      return;
    }
    let prs: GhPR[];
    try {
      prs = listOpenPrs(limit);
    } catch (e: any) {
      console.error(`Error: gh could not list PRs (not a git repo / not authenticated?): ${e.message || e}`);
      process.exitCode = 1;
      return;
    }
    if (prs.length === 0) {
      console.log('No open pull requests.');
      return;
    }

    const nodes = loadGraphNodes(opts.graph);
    const impacts = prs.map((pr) => computeImpact(pr, prFiles(pr.number), nodes));

    printTable(impacts);
    if (opts.conflicts) printConflicts(impacts);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
