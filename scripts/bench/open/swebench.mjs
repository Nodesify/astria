// SWE-bench Verified localization adapter (smoke: django, first N instances).
//
// Protocol (matches the localization evaluations the agent literature
// reports, e.g. LocAgent / Agentless): for each task instance, check out the
// repository at `base_commit`, build the astria graph, query with the issue
// text (`problem_statement`), and rank files by first appearance in the
// NODE output. Gold = files edited by the developer patch (`patch`),
// excluding test files (the test_patch files are the grading harness, not
// the localization target — the convention Agentless uses; both variants
// are recorded).
//
// Grading: file-level hit@k (any gold file in top-k) and MRR of the best
// gold file. This is a retrieval metric only — no execution, no LLM.
//
// Data: princeton-nlp/SWE-bench_Verified (MIT license), fetched via the
// datasets-server rows API into swebench_verified.jsonl by prepare-swebench.py.
// Usage: node scripts/bench/open/swebench.mjs <n> [repo]
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdirSync, rmSync, existsSync } from 'node:fs';
import path from 'node:path';

const repo0 = path.resolve(import.meta.dirname, '..', '..', '..');
const work = path.join(repo0, 'bench-work', 'open');
const cli = path.join(repo0, 'bench-work', 'modes-20260928', 'cli', 'dist', 'index.js');
const N = Number(process.argv[2] || 25);
const repoFilter = process.argv[3] || 'django/django';
const REPO_DIR = path.join(work, 'swe-repos', repoFilter.split('/')[1]);
const OUT_DIR = path.join(work, `swebench-${repoFilter.split('/')[1]}-graphs`);
const env = { ...process.env, NODE_PATH: path.join(repo0, 'node_modules') };
const run = (cmd, args, opts = {}) =>
  spawnSync(cmd, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env, ...opts });

const git = (repo, args) => run('git', ['-C', repo, ...args]);
const all = readFileSync(path.join(work, 'swebench_verified.jsonl'), 'utf8')
  .trim().split('\n').map(JSON.parse)
  .filter(r => r.repo === repoFilter)
  .slice(0, N);

if (!existsSync(REPO_DIR)) throw Error(`clone ${repoFilter} into ${REPO_DIR} first`);

const goldFiles = (patch) => [...new Set(
  patch.split('\n').filter(l => l.startsWith('+++ b/')).map(l => l.slice(6).replaceAll('\\', '/'))
)];
const isTest = (f) => /(^|\/)(tests?|testing)\//.test(f) || /\/test_/.test(f);

const files = (text, cwd) => {
  const out = [];
  for (const line of text.split('\n')) {
    if (!line.startsWith('NODE ')) continue;
    const m = line.match(/\bsrc=(.*?) (?:loc|community)=/);
    if (!m) continue;
    let f = m[1].replace(/:\d+(?::\d+)?$/, '').replaceAll('\\', '/');
    if (path.isAbsolute(f)) f = path.relative(cwd, f).replaceAll('\\', '/');
    if (f && !out.includes(f)) out.push(f);
  }
  return out;
};

// Strip traceback/log noise from an issue body: the retrieval signal is the
// prose and the identifiers, not the pasted stack trace.
const cleanIssue = (text) => {
  const out = [];
  let inTrace = false;
  for (const line of text.split('\n')) {
    if (/Traceback \(most recent call last\)/.test(line)) { inTrace = true; continue; }
    if (inTrace) {
      if (/^\S/.test(line)) inTrace = false; // unindented exception line ends the block
      else continue;
    }
    if (/^\s*File "[^"]+", line \d+/.test(line)) continue;
    out.push(line);
  }
  return out.join('\n').replace(/\n{3,}/g, '\n\n').trim();
};

// Gold function names from the developer patch: hunk headers (@@ ... def f)
// and added definitions (+def f / +async def f).
const goldFuncs = (patch) => [...new Set(
  [...patch.matchAll(/@@.*?\b(?:async\s+)?def\s+([A-Za-z_]\w*)/g),
   ...patch.matchAll(/^\+(?:async\s+)?def\s+([A-Za-z_]\w*)/gm)]
    .map(m => m[1])
)];

const results = [];
for (const [i, inst] of all.entries()) {
  const t0 = Date.now();
  const co = git(REPO_DIR, ['checkout', '--detach', '--force', inst.base_commit]);
  if (co.status !== 0) { results.push({ id: inst.instance_id, error: co.stderr.slice(0, 200) }); continue; }
  rmSync(path.join(REPO_DIR, '.astria'), { recursive: true, force: true });
  const build = run(process.execPath, [cli, 'run', REPO_DIR]);
  if (build.status !== 0) { results.push({ id: inst.instance_id, error: (build.stderr || '').slice(0, 200) }); continue; }
  const goldAll = goldFiles(inst.patch);
  const goldCode = goldAll.filter(f => !isTest(f));
  const funcs = goldFuncs(inst.patch);

  // One graph build, four query configurations: raw vs cleaned issue text,
  // 4k/8k budgets, depth 2/3. Deltas are within-instance comparable.
  const configs = [
    { name: 'base-4k-d2', q: inst.problem_statement, budget: 4000, depth: 2 },
    { name: 'clean-4k-d2', q: cleanIssue(inst.problem_statement), budget: 4000, depth: 2 },
    { name: 'clean-8k-d2', q: cleanIssue(inst.problem_statement), budget: 8000, depth: 2 },
    { name: 'clean-4k-d3', q: cleanIssue(inst.problem_statement), budget: 4000, depth: 3 },
  ];
  const variants = {};
  for (const cfg of configs) {
    const q = run(process.execPath, [cli, 'query', cfg.q, '--budget', String(cfg.budget), '--depth', String(cfg.depth)], { cwd: REPO_DIR });
    const ranked = q.status === 0 ? files(q.stdout, REPO_DIR) : [];
    const rankOf = (gold) => {
      const ranks = gold.map(g => ranked.indexOf(g) + 1).filter(r => r > 0);
      return ranks.length ? Math.min(...ranks) : null;
    };
    const norm = (s) => s.replace(/\(\)$/, '').trim();
    const rankedLabels = [...(q.stdout || '').matchAll(/^NODE (.+?) \[id=/gm)].map(m => norm(m[1]));
    const funcRank = rankedLabels.findIndex(l => funcs.includes(l)) + 1;
    variants[cfg.name] = {
      rank_all: rankOf(goldAll), rank_code: rankOf(goldCode), func_rank: funcRank || null,
      ranked_count: ranked.length,
    };
  }
  results.push({
    id: inst.instance_id,
    base_commit: inst.base_commit,
    gold_all: goldAll, gold_code: goldCode, gold_funcs: funcs,
    variants,
    seconds: +((Date.now() - t0) / 1000).toFixed(1),
  });
  const v = variants['clean-4k-d2'];
  console.log(`[${i + 1}/${all.length}] ${inst.instance_id} clean-4k-d2 rank_code=${v.rank_code} func=${v.func_rank} (${results.at(-1).seconds}s)`);
}

const scored = results.filter(r => r.variants);
const configs = ['base-4k-d2', 'clean-4k-d2', 'clean-8k-d2', 'clean-4k-d3'];
const perConfig = {};
for (const cfg of configs) {
  const mrrOf = (pick) => scored.reduce((s, r) => { const rank = pick(r.variants[cfg]); return s + (rank ? 1 / rank : 0); }, 0) / (scored.length || 1);
  const hitAt = (k, pick) => scored.filter(r => { const rank = pick(r.variants[cfg]); return rank && rank <= k; }).length / (scored.length || 1);
  const fileRank = (v) => v.rank_code;
  const funcRank = (v) => v.func_rank;
  perConfig[cfg] = {
    file_hit1: +hitAt(1, fileRank).toFixed(3),
    file_hit3: +hitAt(3, fileRank).toFixed(3),
    file_hit5: +hitAt(5, fileRank).toFixed(3),
    file_mrr: +mrrOf(fileRank).toFixed(3),
    func_hit1: +hitAt(1, funcRank).toFixed(3),
    func_hit5: +hitAt(5, funcRank).toFixed(3),
    func_mrr: +mrrOf(funcRank).toFixed(3),
  };
}
const summary = {
  benchmark: 'SWE-bench Verified localization',
  subset: `${repoFilter} first ${all.length} instances (smoke)`,
  n: scored.length, errors: results.filter(r => r.error).length,
  configs: perConfig,
  best: configs.reduce((a, b) => (perConfig[b].file_mrr > perConfig[a].file_mrr ? b : a)),
  provenance: { dataset: 'princeton-nlp/SWE-bench_Verified', license: 'MIT', graph: 'structural (no LLM)', note: 'one build per instance, four query configurations graded against dev-patch files (test files excluded) and gold function names from hunk headers' },
};
writeFileSync(path.join(work, `swebench-results-${repoFilter.split('/')[1]}.json`), JSON.stringify({ summary, results }, null, 2));
console.log(JSON.stringify(summary.configs, null, 1));
