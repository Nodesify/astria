// RepoQA find-task adapter (retrieval variant).
//
// Protocol: RepoQA (evalplus, Apache-2.0) "find" task — given a natural
// language description of a function, locate it in a pinned repository.
// The official protocol prompts an LLM with the repo and grades Hit@1 on
// the exact function name. This adapter measures the retrieval substrate:
// build the astria graph for the repo at the pinned commit, query with the
// needle `description`, and grade
//   - file hit: the needle's file (`path`) appears in ranked NODE output
//   - function rank: first ranked NODE whose label matches the function name
// Both are stricter-cursor variants of the official metric (retrieval must
// surface the file/function without an LLM re-ranking step).
//
// Data: bench-work/open/repoqa-2024-06-23.json (dev-dataset release; not
// redistributed). Repos are cloned from their pinned commit_sha.
// Usage: node scripts/bench/open/repoqa.mjs <language> <n_repos>
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, rmSync, mkdirSync, existsSync } from 'node:fs';
import path from 'node:path';

const repo0 = path.resolve(import.meta.dirname, '..', '..', '..');
const work = path.join(repo0, 'bench-work', 'open');
// The working-tree CLI by default (the adapters exist to measure the current
// tool); pin a specific snapshot with ASTRIA_BIN for reproducibility runs.
const cli = process.env.ASTRIA_BIN
  ? path.resolve(process.env.ASTRIA_BIN)
  : path.join(repo0, 'packages', 'astria-cli', 'dist', 'index.js');
const language = process.argv[2] || 'python';
const maxRepos = Number(process.argv[3] || 2);
// EMBED=1 builds each repo with --embed (local embedding model) so the
// description→symbol leg measures semantic recall instead of keyword match.
const embed = process.env.EMBED === '1';
const env = { ...process.env, NODE_PATH: path.join(repo0, 'node_modules') };
const run = (cmd, args, opts = {}) =>
  spawnSync(cmd, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env, ...opts });

const dataset = JSON.parse(readFileSync(path.join(work, 'repoqa-2024-06-23.json'), 'utf8'));
const repos = dataset[language].slice(0, maxRepos);
const reposDir = path.join(work, 'repoqa-repos');

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
const labels = (text) => [...text.matchAll(/^NODE (.+?) \[id=/gm)].map(m => m[1]);

const rows = [];
const failed = []; // a repo that could not be evaluated: the sample is then
// incomplete, which must be loud — a silently shrunk n is not a benchmark.
for (const r of repos) {
  const repoDir = path.join(reposDir, r.repo.replaceAll('/', '__'));
  if (!existsSync(path.join(repoDir, '.git'))) {
    const cl = run('git', ['clone', '--filter=blob:none', `https://github.com/${r.repo}.git`, repoDir]);
    if (cl.status !== 0) { failed.push({ repo: r.repo, stage: 'clone', error: (cl.stderr || '').slice(0, 400) }); continue; }
  }
  const co = run('git', ['-C', repoDir, 'checkout', '--detach', '--force', r.commit_sha]);
  if (co.status !== 0) { failed.push({ repo: r.repo, stage: 'checkout', error: (co.stderr || '').slice(0, 400) }); continue; }
  rmSync(path.join(repoDir, '.astria'), { recursive: true, force: true });
  const build = run(process.execPath, [cli, 'run', repoDir, ...(embed ? ['--embed'] : [])]);
  if (build.status !== 0) { failed.push({ repo: r.repo, stage: 'build', error: (build.stderr || build.stdout || '').slice(-500) }); continue; }

  for (const needle of r.needles) {
    const q = run(process.execPath, [cli, 'query', needle.description, '--budget', '4000', '--depth', '2'], { cwd: repoDir });
    const rankedFiles = q.status === 0 ? files(q.stdout, repoDir) : [];
    const rankedLabels = q.status === 0 ? labels(q.stdout) : [];
    const fileRank = rankedFiles.indexOf(needle.path.replaceAll('\\', '/')) + 1;
    const norm = (s) => s.replace(/\(\)$/, '').trim();
    const funcRank = rankedLabels.findIndex(l => norm(l) === needle.name || l === needle.name) + 1;
    rows.push({
      repo: r.repo, name: needle.name, path: needle.path,
      fileRank: fileRank || null, funcRank: funcRank || null,
      question_tokens: needle.description.length,
    });
    console.log(`${r.repo} ${needle.name} file=${fileRank || 'miss'} func=${funcRank || 'miss'}`);
  }
}

const n = rows.length || 1;
const rate = (key, k) => rows.filter(r => r[key] && r[key] <= k).length / n;
const summary = {
  benchmark: 'RepoQA find (retrieval variant)',
  subset: `${language} first ${repos.length} repos, ${rows.length} needles (smoke)`,
  n: rows.length,
  complete: failed.length === 0,
  failed_repos: failed,
  file_hit1: +rate('fileRank', 1).toFixed(3),
  file_hit5: +rate('fileRank', 5).toFixed(3),
  file_hit10: +rate('fileRank', 10).toFixed(3),
  func_hit1: +rate('funcRank', 1).toFixed(3),
  func_hit5: +rate('funcRank', 5).toFixed(3),
  mrr_file: +(rows.reduce((s, r) => s + (r.fileRank ? 1 / r.fileRank : 0), 0) / n).toFixed(3),
  mrr_func: +(rows.reduce((s, r) => s + (r.funcRank ? 1 / r.funcRank : 0), 0) / n).toFixed(3),
  budget: 4000, depth: 2,
  embed,
  provenance: { dataset: 'evalplus/repoqa dev-dataset 2024-06-23', license: 'Apache-2.0', protocol_note: 'official task grades LLM Hit@1 on the function given the repo; this measures the graph-retrieval substrate without the LLM step', ...(embed ? { embed: 'local embedding model, similar_to edges + semantic seeds active' } : {}) },
};
writeFileSync(path.join(work, `repoqa-results-${language}${embed ? '-embed' : ''}.json`), JSON.stringify({ summary, rows }, null, 2));
console.log(JSON.stringify(summary, null, 1));
if (failed.length) {
  console.error(`\nREPOQA INCOMPLETE: ${failed.length}/${repos.length} repos failed — these numbers are NOT a valid sample:`);
  for (const f of failed) console.error(`  [${f.stage}] ${f.repo}: ${f.error.split('\n')[0]}`);
  process.exitCode = 1;
}
