// Explicit opt-in evaluation: pinned source, structural build, three methods.
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadTokenizer } from '../tokenize.mjs';
const dir = path.dirname(fileURLToPath(import.meta.url)), repo = path.resolve(dir, '../../..');
let includeReserved = false, output = path.join(repo, 'bench-work/external');
for (let index = 2; index < process.argv.length; index++) {
  const arg = process.argv[index];
  if (arg === '--help' || arg === '-h') {
    console.log('Usage: node scripts/bench/external/run.mjs [--output NEW_DIRECTORY] [--include-reserved]. Clones pinned source only, builds structural graphs, compares graph / single-pass floor v2 / iterative search v1 at 1000 and 4000 tokens. Reserved questions are skipped unless explicitly included; first query is logged as exposure. Never runs upstream code.');
    process.exit(0);
  } else if (arg === '--include-reserved') includeReserved = true;
  else if (arg === '--output' && process.argv[index + 1]) output = path.resolve(process.argv[++index]);
  else throw Error('Unknown or incomplete argument: ' + arg);
}
if (existsSync(output)) throw Error('Output must be a new directory: ' + output);
const cli = path.join(repo, 'packages/astria-cli/dist/index.js'), native = path.join(repo, 'packages/astria-cli/dist/astria.node');
if (!await loadTokenizer()) throw Error('Suite requires js-tiktoken (o200k_base)');
if (!existsSync(cli) || !existsSync(native) || existsSync(path.join(repo, 'packages/astria-cli/astria.node'))) throw Error('Build dist/index.js and dist/astria.node; package-root astria.node must not exist');
const env = { ...process.env, ASTRIA_LLM_BACKEND: 'none' };
const run = (cmd, args, cwd = repo) => {
  const start = performance.now();
  const result = spawnSync(cmd, args, { cwd, env, encoding: 'utf8', timeout: 600000, maxBuffer: 64 * 1024 * 1024 });
  const seconds = (performance.now() - start) / 1000;
  if (result.status !== 0 || result.error) throw Error(cmd + ' failed: ' + (result.error?.message || result.stderr || result.status));
  return { stdout: result.stdout.trim(), stderr: result.stderr || '', seconds };
};
const sha = file => createHash('sha256').update(readFileSync(file)).digest('hex');
// No text decoding/trim: checkout line endings must not affect source identity.
const gitBlobSha = (root, commit, file) => {
  const result = spawnSync('git', ['show', commit + ':' + file], { cwd: root, env, timeout: 30000, maxBuffer: 64 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw Error('Cannot read pinned Git blob: ' + file + ': ' + (result.error?.message || result.stderr?.toString('utf8') || result.status));
  return createHash('sha256').update(result.stdout).digest('hex');
};
mkdirSync(output, { recursive: true });
const suite = { schema_version: 1, generated_at: new Date().toISOString(), provenance: { harness_commit: run('git', ['rev-parse', 'HEAD']).stdout, harness_status: run('git', ['status', '--porcelain']).stdout, cli_version: run(process.execPath, [cli, '--version']).stdout, cli_sha256: sha(cli), native_sha256: sha(native), node: process.version, platform: process.platform }, policy: { budgets: [1000, 4000], depth: 3, structural: true, backend: 'none', embeddings: false, build_cost: 'separate per-corpus wall time, includes CLI post-build work', query_repetitions: 1, reserved_opt_in: includeReserved }, corpora: [] };
const save = () => writeFileSync(path.join(output, 'suite.json'), JSON.stringify(suite, null, 2) + '\n');
save();
for (const corpus of JSON.parse(readFileSync(path.join(dir, 'corpora.json'), 'utf8'))) {
  if (corpus.split === 'reserved' && !includeReserved) continue;
  const root = path.join(output, corpus.name);
  const row = { ...corpus, golden_sha256: sha(path.join(dir, corpus.golden)), exposure: null, build: null, results: [] };
  suite.corpora.push(row); save();
  if (!/^[0-9a-f]{40}$/.test(corpus.commit)) throw Error('Full commit pin required');
  run('git', ['clone', '--no-checkout', corpus.repository, root]);
  run('git', ['-C', root, 'checkout', '--detach', corpus.commit]);
  if (run('git', ['rev-parse', 'HEAD'], root).stdout !== corpus.commit || run('git', ['status', '--porcelain', '--untracked-files=all'], root).stdout) throw Error('Corpus must be clean at its pin');
  for (const [file, expected] of Object.entries(corpus.source_sha256 || {})) {
    if (gitBlobSha(root, corpus.commit, file) !== expected) throw Error('Pinned source blob hash mismatch: ' + corpus.name + '/' + file);
  }
  const items = readFileSync(path.join(dir, corpus.golden), 'utf8').trim().split('\n').map(JSON.parse);
  for (const item of items) {
    for (const evidence of item.evidence) if (!readFileSync(path.join(root, evidence.path), 'utf8').includes(evidence.contains)) throw Error('Ungrounded evidence: ' + evidence.path);
    for (const definition of item.definitions || []) if (!readFileSync(path.join(root, definition.path), 'utf8').split(/\r?\n/)[definition.line - 1]?.includes(definition.contains)) throw Error('Ungrounded declaration: ' + item.id);
  }
  const build = run(process.execPath, [cli, 'run', '.'], root);
  row.build = { seconds: build.seconds, baseline_build_seconds: 0, stdout: build.stdout, stderr: build.stderr };
  save();
  for (const budget of [1000, 4000]) for (const method of ['astria', 'rg-single-pass-v2', 'rg-iterative-v1']) {
    if (corpus.split === 'reserved' && !row.exposure) { row.exposure = { started_at: new Date().toISOString(), status: 'reservation-consumed-before-first-query' }; save(); }
    const resultPath = path.join(output, corpus.name + '-' + budget + '-' + method + '.json');
    run(process.execPath, [path.join(repo, 'scripts/bench/quality/run-quality.mjs'), '--root', root, '--golden', path.join(dir, corpus.golden), '--astria', cli, '--budget', String(budget), '--depth', '3', '--out', resultPath, ...(method === 'rg-single-pass-v2' ? ['--baseline'] : method === 'rg-iterative-v1' ? ['--iterative-baseline'] : []), ...(corpus.split === 'reserved' ? ['--allow-reserved'] : [])]);
    row.results.push({ method, budget, path: path.basename(resultPath) }); save();
  }
}
console.log('Suite: ' + path.join(output, 'suite.json'));
