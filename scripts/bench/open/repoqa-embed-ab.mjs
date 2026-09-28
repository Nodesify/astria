// A/B: RepoQA needle retrieval with and without local embeddings.
// Graphs must already carry embeddings (run `astria update <repo> --embed`
// first); ASTRIA_EMBED=off reproduces the no-embed arm per query.
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';

const repo0 = path.resolve(import.meta.dirname, '..', '..', '..');
const work = path.join(repo0, 'bench-work', 'open');
const cli = path.join(repo0, 'bench-work', 'modes-20260928', 'cli', 'dist', 'index.js');
const env = { ...process.env, NODE_PATH: path.join(repo0, 'node_modules') };
const run = (cmd, args, opts = {}) =>
  spawnSync(cmd, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env, ...opts });

const dataset = JSON.parse(readFileSync(path.join(work, 'repoqa-2024-06-23.json'), 'utf8'));
const repos = process.argv.slice(2).length ? process.argv.slice(2) : ['psf/black'];
const norm = (s) => s.replace(/\(\)$/, '').trim();

const rows = [];
for (const repo of repos) {
  const repoDir = path.join(work, 'repoqa-repos', repo.replaceAll('/', '__'));
  const r = dataset[dataset['python'].some(x => x.repo === repo) ? 'python' : null];
  if (!r) { console.error(`repo not in python dataset: ${repo}`); continue; }
  const spec = r.find(x => x.repo === repo);
  for (const arm of ['embed', 'noembed']) {
    for (const needle of spec.needles) {
      const envArm = { ...env, ASTRIA_EMBED: arm === 'embed' ? 'on' : 'off' };
      const q = spawnSync(process.execPath, [cli, 'query', needle.description, '--budget', '4000', '--depth', '2'],
        { cwd: repoDir, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env: envArm });
      const labels = [...q.stdout.matchAll(/^NODE (.+?) \[id=/gm)].map(m => m[1]);
      const funcRank = labels.findIndex(l => norm(l) === needle.name || l === needle.name) + 1;
      rows.push({ repo, name: needle.name, arm, funcRank: funcRank || null });
    }
  }
}

const sum = (arm) => {
  const rs = rows.filter(r => r.arm === arm);
  const n = rs.length || 1;
  return {
    n: rs.length,
    func_hit1: +(rs.filter(r => r.funcRank === 1).length / n).toFixed(3),
    func_hit5: +(rs.filter(r => r.funcRank && r.funcRank <= 5).length / n).toFixed(3),
    mrr_func: +(rs.reduce((s, r) => s + (r.funcRank ? 1 / r.funcRank : 0), 0) / n).toFixed(3),
  };
};
console.log(JSON.stringify({ noembed: sum('noembed'), embed: sum('embed') }, null, 1));
const improved = rows.filter(r => r.arm === 'embed' && r.funcRank && (!rows.find(x => x.arm === 'noembed' && x.name === r.name && x.repo === r.repo).funcRank || rows.find(x => x.arm === 'noembed' && x.name === r.name && x.repo === r.repo).funcRank > r.funcRank));
for (const r of improved) console.log(`embed improved: ${r.repo} ${r.name} -> ${r.funcRank}`);
