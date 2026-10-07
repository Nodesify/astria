// Read-only GitHub review queue. Every PR uses the same revision-aware report
// as 'risk' and 'merge-gate'; missing Git objects or API data stay unavailable.
import { execFileSync } from 'child_process';
import { resolve } from 'path';
import { changeReview, ChangeReview } from './change-review';

interface GhCheck {
  name?: string; workflowName?: string; conclusion?: string | null;
  status?: string | null; state?: string | null;
}
interface GhPR {
  number: number; title: string; headRefName: string;
  baseRefOid: string; headRefOid: string; isDraft: boolean;
  author?: { login?: string }; additions?: number; deletions?: number;
  reviewDecision?: string | null; statusCheckRollup?: GhCheck[];
  mergeable?: string; mergeStateStatus?: string;
}
interface PrReview {
  number: number; title: string; author: string; draft: boolean;
  baseCommit: string; headCommit: string; ci: { state: string; failing: string[] };
  reviewState: string; mergeable: string; mergeState: string;
  worktree: string | null; size: number; files: string[];
  review: ChangeReview | null; coverageError: string | null;
  queueScore: number; reasons: string[];
}

function gh(root: string, args: string[]): string {
  return execFileSync('gh', args, { cwd: root, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] });
}
function errorText(error: unknown): string {
  const e = error as { stderr?: Buffer | string; message?: string };
  return String(e.stderr || e.message || error).trim();
}
function checks(pr: GhPR): PrReview['ci'] {
  let state = 'none';
  const failing: string[] = [];
  for (const check of pr.statusCheckRollup ?? []) {
    const verdict = check.conclusion || check.state || check.status || '';
    if (/^(FAILURE|ERROR|FAILED|TIMED_OUT|CANCELLED|ACTION_REQUIRED|STARTUP_FAILURE)$/.test(verdict)) {
      state = 'failing'; failing.push(check.name ?? check.workflowName ?? 'check');
    } else if (/^(PENDING|IN_PROGRESS|QUEUED|REQUESTED|WAITING|EXPECTED)$/.test(verdict) && state !== 'failing') {
      state = 'running';
    } else if (verdict === 'SUCCESS' && state === 'none') state = 'passing';
  }
  return { state, failing };
}
function worktrees(root: string): Map<string, string> {
  const result = new Map<string, string>();
  try {
    const output = execFileSync('git', ['worktree', 'list', '--porcelain', '-z'], { cwd: root, encoding: 'utf8', stdio: 'pipe' });
    let path = '';
    for (const field of output.split('\0')) {
      if (field.startsWith('worktree ')) path = field.slice(9);
      else if (field.startsWith('branch refs/heads/')) result.set(field.slice(18), path);
    }
  } catch { /* Optional navigation aid; never used as impact evidence. */ }
  return result;
}

function reviewPr(root: string, pr: GhPR, trees: Map<string, string>): PrReview {
  const result: PrReview = {
    number: pr.number, title: pr.title, author: pr.author?.login ?? 'unknown', draft: pr.isDraft,
    baseCommit: pr.baseRefOid, headCommit: pr.headRefOid, ci: checks(pr),
    reviewState: pr.reviewDecision || 'unreviewed', mergeable: pr.mergeable || 'UNKNOWN',
    mergeState: pr.mergeStateStatus || 'UNKNOWN', worktree: trees.get(pr.headRefName) ?? null,
    size: (pr.additions ?? 0) + (pr.deletions ?? 0), files: [], review: null,
    coverageError: null, queueScore: 0, reasons: [],
  };
  try {
    if (!/^[0-9a-f]{40,64}$/i.test(pr.baseRefOid) || !/^[0-9a-f]{40,64}$/i.test(pr.headRefOid)) {
      throw new Error('GitHub did not return immutable base/head commit IDs');
    }
    // Keep API failure distinct from an empty patch. Checking the returned head
    // also catches a PR moving between the list and detail requests.
    const detail: { headRefOid?: string; files?: Array<{ path: string }> } =
      JSON.parse(gh(root, ['pr', 'view', String(pr.number), '--json', 'files,headRefOid']));
    if (!Array.isArray(detail.files) || detail.headRefOid !== pr.headRefOid) {
      throw new Error('PR files are unavailable or the PR head changed during review; retry');
    }
    result.files = detail.files.map((file) => file.path);
    result.review = changeReview({ graph: root, base: pr.baseRefOid, head: pr.headRefOid });
    // The Git comparison is authoritative; API lists may be truncated on very
    // large PRs, but a file known by the API must exist in the analyzed diff.
    const localPaths = new Set(result.review.changedFiles.flatMap((file) => [file.oldPath, file.newPath]).filter(Boolean));
    if (result.files.some((path) => !localPaths.has(path))) {
      throw new Error('GitHub file list disagrees with the local immutable revision comparison');
    }
    if (!result.review.coverageComplete) result.coverageError = result.review.coverageIssues.join('; ');
  } catch (error) {
    result.coverageError = `Impact unavailable: ${errorText(error)}. Ensure both PR commits and their merge base are available locally.`;
    result.review = null;
  }
  let score = 0;
  if (result.coverageError) { score += 35; result.reasons.push('impact coverage requires attention'); }
  if (result.ci.state === 'failing') { score += 40; result.reasons.push(`CI failing: ${result.ci.failing.join(', ')}`); }
  if (result.reviewState === 'CHANGES_REQUESTED') { score += 30; result.reasons.push('changes requested'); }
  if (result.mergeable === 'CONFLICTING') { score += 25; result.reasons.push('GitHub reports merge conflicts'); }
  if (result.review) {
    score += Math.min(result.review.impacted, 30);
    result.reasons.push(`${result.review.directConsumers} direct consumers, ${result.review.testConsumers} related tests`);
  }
  if (result.draft) { score -= 20; result.reasons.unshift('draft'); }
  result.queueScore = score;
  return result;
}

function overlaps(reviews: PrReview[]): Array<{ left: number; right: number; symbols: string[] }> {
  const output: Array<{ left: number; right: number; symbols: string[] }> = [];
  for (let i = 0; i < reviews.length; i++) {
    for (let j = i + 1; j < reviews.length; j++) {
      const left = reviews[i].review, right = reviews[j].review;
      if (!left || !right) continue;
      const touched = (r: ChangeReview) => new Set([...r.declarations.map((d) => d.id), ...r.consumers.map((c) => c.id)]);
      const a = touched(left), b = touched(right);
      const symbols = [...a].filter((id) => b.has(id));
      if (symbols.length) output.push({ left: reviews[i].number, right: reviews[j].number, symbols });
    }
  }
  return output;
}

export async function prsCommand(count: string, opts: { graph: string; conflicts: boolean; triage?: boolean; queue?: boolean; json?: boolean }) {
  try {
    const limit = Number(count);
    if (!Number.isInteger(limit) || limit < 1 || limit > 100) throw new Error('PR count must be an integer from 1 to 100');
    const root = resolve(opts.graph);
    const fields = 'number,title,headRefName,baseRefOid,headRefOid,isDraft,author,additions,deletions,reviewDecision,statusCheckRollup,mergeable,mergeStateStatus';
    const prs: GhPR[] = JSON.parse(gh(root, ['pr', 'list', '--limit', String(limit), '--json', fields]));
    const trees = worktrees(root);
    const ranked = prs.map((pr) => reviewPr(root, pr, trees)).sort((a,b) => b.queueScore - a.queueScore || a.number - b.number);
    const shared = opts.conflicts ? overlaps(ranked) : [];
    if (opts.json) {
      console.log(JSON.stringify({ pullRequests: ranked, reviewOverlap: shared, overlapMeaning: 'Shared changed/affected symbols are a review coordination signal, not proof of a merge conflict.' }, null, 2));
    } else if (ranked.length === 0) {
      console.log('No open pull requests.');
    } else {
      console.log('Review queue — ranked by attention needed\n');
      for (const [index, pr] of ranked.entries()) {
        console.log(`${index + 1}. #${pr.number} ${pr.title}${pr.draft ? ' [draft]' : ''} — ${pr.reasons.join('; ') || 'no priority signals'}`);
        console.log(`   CI: ${pr.ci.state}; review: ${pr.reviewState}; GitHub merge status: ${pr.mergeable}`);
        if (pr.coverageError) console.log(`   ${pr.coverageError}`);
        if (!opts.triage && !opts.queue && pr.review) console.log(`\n${pr.review.text}`);
      }
      if (opts.conflicts) {
        console.log('\nReview overlap — shared symbols; not proof of a merge conflict');
        for (const item of shared) console.log(`#${item.left} ↔ #${item.right}: ${item.symbols.join(', ')}`);
        if (!shared.length) console.log('No shared changed/affected symbols in available reports.');
      }
    }
    if (ranked.some((pr) => pr.coverageError)) process.exitCode = 1;
  } catch (error) {
    console.error(`Error: ${errorText(error)}`);
    process.exitCode = 1;
  }
}
