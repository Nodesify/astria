// PR impact dashboard: maps open PRs' changed files onto the knowledge
// graph (communities touched, node blast radius), pulls CI + review state,
// maps branches onto git worktrees, ranks the review queue, and flags
// merge-order risk. Requires the `gh` CLI (read-only).
//
// Richer than the v1 port of upstream graphify's prs.py: one `gh pr list`
// call carries CI (statusCheckRollup) and review (reviewDecision) state, so
// the dashboard ranks the queue instead of just listing it.

import { execFileSync } from 'child_process';
import { existsSync, readFileSync } from 'fs';
import { join } from 'path';

interface GhCheck {
  __typename?: string;
  name?: string;
  conclusion?: string | null;
  status?: string | null;
  state?: string | null;
  workflowName?: string | null;
}

interface GhPR {
  number: number;
  title: string;
  headRefName: string;
  baseRefName: string;
  isDraft: boolean;
  author?: { login?: string };
  createdAt?: string;
  updatedAt?: string;
  additions?: number;
  deletions?: number;
  reviewDecision?: string | null;
  statusCheckRollup?: GhCheck[];
  mergeable?: string;
  mergeStateStatus?: string;
}

interface Worktree {
  branch: string | null;
  path: string;
}

interface PrImpact {
  number: number;
  title: string;
  author: string;
  draft: boolean;
  files: string[];
  nodes: Set<string>;
  communities: Map<number, number>; // community -> node count
  ci: { state: string; failing: string[] };
  review: string;
  mergeable: string;
  mergeState: string;
  worktree: string | null;
  size: number;
  riskScore: number;
  reasons: string[];
}

const DIM = '\x1b[2m';
const BOLD = '\x1b[1m';
const GREEN = '\x1b[32m';
const YELLOW = '\x1b[33m';
const RED = '\x1b[31m';
const CYAN = '\x1b[36m';
const RESET = '\x1b[0m';

function gh(args: string[]): string {
  return execFileSync('gh', args, { encoding: 'utf-8', stdio: ['pipe', 'pipe', 'pipe'] });
}

function ghAvailable(): boolean {
  try {
    execFileSync('gh', ['--version'], { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}

const PR_FIELDS =
  'number,title,headRefName,baseRefName,isDraft,author,createdAt,updatedAt,additions,deletions,reviewDecision,statusCheckRollup,mergeable,mergeStateStatus';

function listOpenPrs(limit: number): GhPR[] {
  return JSON.parse(gh(['pr', 'list', '--limit', String(limit), '--json', PR_FIELDS]));
}

function prFiles(number: number): string[] {
  if (!Number.isInteger(number) || number <= 0) return [];
  try {
    const parsed = JSON.parse(gh(['pr', 'view', String(number), '--json', 'files']));
    return (parsed.files || []).map((f: { path: string }) => f.path);
  } catch {
    return [];
  }
}

function listWorktrees(): Worktree[] {
  try {
    const out = execFileSync('git', ['worktree', 'list', '--porcelain'], {
      encoding: 'utf-8',
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    const trees: Worktree[] = [];
    let path = '';
    for (const line of out.split('\n')) {
      if (line.startsWith('worktree ')) path = line.slice('worktree '.length);
      else if (line.startsWith('branch ')) trees.push({ branch: line.slice('branch '.length).replace('refs/heads/', ''), path });
      else if (line === 'detached' || line === 'bare') trees.push({ branch: null, path });
    }
    return trees;
  } catch {
    return [];
  }
}

function summarizeChecks(pr: GhPR): PrImpact['ci'] {
  const checks = pr.statusCheckRollup ?? [];
  const failing: string[] = [];
  let state = 'none';
  for (const check of checks) {
    // The rollup mixes CheckRun and StatusContext shapes; both carry a
    // conclusion/state, one of the two is always set.
    const verdict = check.conclusion ?? check.state ?? check.status ?? '';
    const name = check.name ?? check.workflowName ?? 'check';
    if (/FAILURE|ERROR|FAILED|EXPECTING/.test(verdict)) {
      failing.push(name);
      state = 'failing';
    } else if (/PENDING|IN_PROGRESS|QUEUED/.test(verdict) && state !== 'failing') {
      state = 'running';
    } else if (verdict === 'SUCCESS' && state === 'none') {
      state = 'passing';
    }
  }
  return { state, failing };
}

function loadGraphNodes(graphRoot: string): { source_file: string; community: number | null }[] {
  const graphJson = join(graphRoot, '.astria', 'graph.json');
  if (!existsSync(graphJson)) {
    throw new Error(`no graph found at ${graphJson} — run 'astria run ${graphRoot}' first`);
  }
  const graph = JSON.parse(readFileSync(graphJson, 'utf-8'));
  return (graph.nodes || []).map((n: any) => ({
    source_file: String(n.source_file || '').replace(/\\/g, '/'),
    community: n.community ?? null,
  }));
}

function computeImpact(
  pr: GhPR,
  files: string[],
  nodes: { source_file: string; community: number | null }[],
  worktrees: Worktree[],
): PrImpact {
  const normalized = files.map((f) => f.replace(/\\/g, '/'));
  const impact: PrImpact = {
    number: pr.number,
    title: pr.title,
    author: pr.author?.login ?? 'unknown',
    draft: pr.isDraft,
    files,
    nodes: new Set(),
    communities: new Map(),
    ci: summarizeChecks(pr),
    review: pr.reviewDecision || 'unreviewed',
    mergeable: pr.mergeable || 'UNKNOWN',
    mergeState: pr.mergeStateStatus || 'UNKNOWN',
    worktree: worktrees.find((w) => w.branch === pr.headRefName)?.path ?? null,
    size: (pr.additions ?? 0) + (pr.deletions ?? 0),
    riskScore: 0,
    reasons: [],
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

/// Rank the review queue: the score is urgency, not danger — what should a
/// reviewer look at first? Failing CI and requested changes rise to the top,
/// then graph impact (communities touched = blast radius), then size.
function rankQueue(impacts: PrImpact[]): PrImpact[] {
  for (const pr of impacts) {
    let score = 0;
    const reasons: string[] = [];
    if (pr.ci.state === 'failing') {
      score += 40;
      reasons.push(`CI failing (${pr.ci.failing.slice(0, 2).join(', ')})`);
    }
    if (pr.review === 'CHANGES_REQUESTED') {
      score += 30;
      reasons.push('changes requested');
    }
    const symbols = [...pr.communities.values()].reduce((a, b) => a + b, 0);
    score += Math.min(symbols, 60) / 2; // up to +30 from graph blast radius
    const communities = pr.communities.size;
    if (communities >= 3) {
      score += 10;
      reasons.push(`spans ${communities} communities`);
    }
    if (pr.mergeable === 'CONFLICTING') {
      score += 25;
      reasons.push('merge conflicts');
    }
    if (pr.draft) {
      score -= 20;
      reasons.unshift('draft');
    }
    if (pr.review === 'APPROVED' && pr.ci.state === 'passing') {
      score += 15;
      reasons.push('ready to merge');
    }
    pr.riskScore = score;
    pr.reasons = reasons;
  }
  return [...impacts].sort((a, b) => b.riskScore - a.riskScore || a.number - b.number);
}

function ciBadge(pr: PrImpact): string {
  switch (pr.ci.state) {
    case 'passing': return `${GREEN}ci✓${RESET}`;
    case 'failing': return `${RED}ci✗${RESET}`;
    case 'running': return `${YELLOW}ci…${RESET}`;
    default: return `${DIM}ci−${RESET}`;
  }
}

function reviewBadge(pr: PrImpact): string {
  switch (pr.review) {
    case 'APPROVED': return `${GREEN}approved${RESET}`;
    case 'CHANGES_REQUESTED': return `${RED}changes-requested${RESET}`;
    case 'REVIEW_REQUIRED': return `${YELLOW}review-required${RESET}`;
    default: return `${DIM}unreviewed${RESET}`;
  }
}

function mergeBadge(pr: PrImpact): string {
  if (pr.mergeable === 'CONFLICTING') return `${RED}conflicts${RESET}`;
  if (pr.mergeState === 'BLOCKED' || pr.mergeState === 'UNSTABLE') return `${YELLOW}${pr.mergeState.toLowerCase()}${RESET}`;
  if (pr.mergeState === 'CLEAN' || pr.mergeable === 'MERGEABLE') return `${GREEN}clean${RESET}`;
  return `${DIM}unknown${RESET}`;
}

function printTable(impacts: PrImpact[]): void {
  console.log(`${BOLD}Open pull requests mapped onto the knowledge graph${RESET}\n`);
  const numW = String(Math.max(...impacts.map((i) => i.number), 0)).length;
  for (const pr of impacts) {
    const draft = pr.draft ? `${DIM}[draft]${RESET} ` : '';
    const top = [...pr.communities.entries()].sort((a, b) => b[1] - a[1]).slice(0, 4);
    const commStr = top.map(([c, n]) => `${CYAN}${c}${RESET}:${n}`).join(' ') || '—';
    const symbols = [...pr.communities.values()].reduce((a, b) => a + b, 0);
    const worktree = pr.worktree ? ` ${DIM}(worktree: ${pr.worktree})${RESET}` : '';
    console.log(`${DIM}#${String(pr.number).padStart(numW)}${RESET} ${draft}${pr.title}  ${DIM}@${pr.author}${RESET}`);
    console.log(
      `     ${ciBadge(pr)} ${reviewBadge(pr)} ${mergeBadge(pr)} · ${DIM}${pr.files.length} file(s), ±${pr.size}${RESET} · ${symbols} graph symbols · communities: ${commStr}${worktree}`,
    );
  }
}

/// The ranked review queue: one line per PR, most urgent first, with the
/// reasons the ranker chose it.
function printQueue(impacts: PrImpact[]): void {
  console.log(`\n${BOLD}Review queue (ranked by urgency)${RESET}\n`);
  impacts.forEach((pr, i) => {
    const reasons = pr.reasons.length ? ` — ${pr.reasons.slice(0, 3).join(', ')}` : '';
    console.log(
      `${String(i + 1).padStart(2)}. ${DIM}#${pr.number}${RESET} ${pr.title} ${DIM}(${Math.round(pr.riskScore)} pts)${RESET}${reasons}`,
    );
  });
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

export async function prsCommand(
  count: string,
  opts: { graph: string; conflicts: boolean; triage?: boolean; queue?: boolean; json?: boolean },
) {
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
    const worktrees = listWorktrees();
    const impacts = prs.map((pr) => computeImpact(pr, prFiles(pr.number), nodes, worktrees));
    const ranked = rankQueue(impacts);

    if (opts.json) {
      console.log(JSON.stringify(ranked.map((pr) => ({
        number: pr.number,
        title: pr.title,
        author: pr.author,
        draft: pr.draft,
        ci_state: pr.ci.state,
        ci_failing: pr.ci.failing,
        review: pr.review,
        mergeable: pr.mergeable,
        merge_state: pr.mergeState,
        worktree: pr.worktree,
        files: pr.files.length,
        symbols_touched: [...pr.communities.values()].reduce((a, b) => a + b, 0),
        communities: Object.fromEntries(pr.communities),
        queue_score: Math.round(pr.riskScore),
        reasons: pr.reasons,
      })), null, 2));
      return;
    }

    if (opts.triage) {
      console.log(`${BOLD}PR triage${RESET}\n`);
      for (const pr of ranked) {
        console.log(
          `${ciBadge(pr)} ${DIM}#${pr.number}${RESET} ${pr.title}`,
        );
        console.log(
          `   ${DIM}${reviewBadge(pr)} · ${mergeBadge(pr)} · ${pr.reasons.join(' · ') || 'no signals'}${RESET}`,
        );
      }
      if (opts.conflicts) printConflicts(ranked);
      return;
    }

    if (opts.queue) {
      printQueue(ranked);
      if (opts.conflicts) printConflicts(ranked);
      return;
    }

    printTable(ranked);
    printQueue(ranked);
    if (opts.conflicts) printConflicts(ranked);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
