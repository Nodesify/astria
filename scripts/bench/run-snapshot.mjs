// One-shot benchmark snapshot: original graphify vs astria on the
// same corpus (safishamsi/graphify @91f4d12), same machine (the CI runner).
// Writes website/src/data/benchmarks-snapshot.json; the workflow commits it.
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { corpusTokensExact, countTokens, loadTokenizer } from './tokenize.mjs';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const work = path.join(repoRoot, 'bench-work');
const corpus = path.join(work, 'corpus');
const venv = path.join(work, 'venv');
const isWin = process.platform === 'win32';
const venvPython = isWin ? path.join(venv, 'Scripts', 'python.exe') : path.join(venv, 'bin', 'python');
const venvGraphify = isWin ? path.join(venv, 'Scripts', 'graphify.exe') : path.join(venv, 'bin', 'graphify');
const ORIG_REPO = 'https://github.com/safishamsi/graphify';
const ORIG_COMMIT = '91f4d12';

// The five questions BOTH tools answer for the parity block — same questions,
// same budget, answers counted with the SAME tokenizer.
const PARITY_QUESTIONS = [
  'how does authentication work',
  'what is the main entry point',
  'how are errors handled',
  'data layer api',
  'core abstractions',
];
const PARITY_BUDGET = '4000';

const sh = (cmd, args, opts = {}) =>
  execFileSync(cmd, args, { stdio: ['ignore', 'pipe', 'inherit'], encoding: 'utf8', ...opts });
const run = (cmd, args, opts = {}) => spawnSync(cmd, args, { encoding: 'utf8', ...opts });
const timed = (cmd, args, opts = {}) => {
  const t0 = Date.now();
  const r = run(cmd, args, opts);
  return { seconds: (Date.now() - t0) / 1000, r };
};
const num = (s) => Number(String(s).replace(/,/g, ''));

// 0. clean workspace
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });

// 1. corpus
sh('git', ['clone', '--quiet', ORIG_REPO, 'corpus'], { cwd: work });
sh('git', ['checkout', '--quiet', ORIG_COMMIT], { cwd: corpus });

// 2. venv for the original
sh('uv', ['venv', '--python', '3.12', 'venv'], { cwd: work });
sh('uv', ['pip', 'install', '--python', venvPython, '--quiet', '-e', './corpus'], { cwd: work });
const pythonVersion = sh(venvPython, ['--version']).trim();

// 3. original: structural pipeline + its own benchmark
const origRun = path.join(work, 'orig-results.json');
sh(venvPython, [path.join(repoRoot, 'scripts', 'bench', 'orig_run.py'), corpus, origRun]);
const origResults = JSON.parse(readFileSync(origRun, 'utf8'));
const origBenchOut = sh(venvGraphify, ['benchmark', 'graphify-out/graph.json'], { cwd: corpus });
const origCorpusTokens = origBenchOut.match(/Corpus:\s+[\d,]+ words → ~([\d,]+) tokens/);
const origAvgQuery = origBenchOut.match(/Avg query cost:\s+~([\d,]+) tokens/);
const origReduction = origBenchOut.match(/Reduction:\s+([\d.]+)x/);

// 3b. original: answer the shared parity questions so both sides can be
// counted with the SAME tokenizer. Best effort — `query` may not exist at
// the pinned commit; a null lands in the parity block with a note.
const origParityAnswers = [];
for (const q of PARITY_QUESTIONS) {
  const r = run(venvGraphify, ['query', q], { cwd: corpus });
  if (r.status === 0 && r.stdout) origParityAnswers.push(r.stdout);
}
const origParityAvg = origParityAnswers.length
  ? Math.round(origParityAnswers.reduce((s, t) => s + countTokens(t, tok), 0) / origParityAnswers.length)
  : null;

// 4. clean the original's artifacts so ours sees the same pristine corpus
for (const p of ['graphify-out', '.graphify_detect.json', '.graphify_ast.json', '.graphify_extract.json']) {
  rmSync(path.join(corpus, p), { recursive: true, force: true });
}

// 5. ours: structural run (no embed, no LLM) + stats
const oursRun = timed('astria', ['run', '.'], { cwd: corpus });
if (oursRun.r.status !== 0) {
  console.error(oursRun.r.stdout, oursRun.r.stderr);
  process.exit(1);
}
const oursOut = oursRun.r.stdout ?? '';
const oursCorpusTokens = oursOut.match(/Corpus:\s+.*~([\d,]+) tokens/);
const oursAvgQuery = oursOut.match(/Avg query cost:\s+~([\d,]+) tokens/);
const oursReduction = oursOut.match(/Reduction:\s+([\d.]+)x/);
const oursGraph = oursOut.match(/Graph:\s+([\d,]+) nodes, ([\d,]+) edges/);
const statsOut = sh('astria', ['stats', '--graph', '.'], { cwd: corpus });
const stat = (re) => (statsOut.match(re) ?? [])[1];
const oursVersion = sh('astria', ['--version']).trim();

// 5b. ours: the same parity questions through the same query engine.
const oursParityAnswers = [];
for (const q of PARITY_QUESTIONS) {
  const r = run('astria', ['query', q, '--budget', PARITY_BUDGET], { cwd: corpus });
  if (r.status === 0 && r.stdout) oursParityAnswers.push(r.stdout);
}
const oursParityAvg = oursParityAnswers.length
  ? Math.round(oursParityAnswers.reduce((s, t) => s + countTokens(t, tok), 0) / oursParityAnswers.length)
  : null;

// Exact corpus tokens with ONE tokenizer — the number both parity
// reductions divide. Skips build artifacts via tokenize.mjs's skip list.
const parityCorpus = await corpusTokensExact(corpus, tok);

// 6. assemble snapshot
const snapshot = {
  generated_at: new Date().toISOString(),
  runner: process.env.BENCH_RUNNER_LABEL ?? 'local machine',
  corpus: {
    repo: ORIG_REPO,
    commit: ORIG_COMMIT,
    files_detected: origResults.files_detected,
  },
  versions: {
    astria: oursVersion,
    original: `graphify @ ${ORIG_COMMIT}`,
    python: pythonVersion,
  },
  original_tool: {
    build_seconds: origResults.build_seconds,
    nodes: origResults.nodes,
    edges: origResults.edges,
    communities: origResults.communities,
    benchmark: {
      corpus_tokens: origCorpusTokens ? num(origCorpusTokens[1]) : null,
      avg_query_tokens: origAvgQuery ? num(origAvgQuery[1]) : null,
      reduction: origReduction ? Number(origReduction[1]) : null,
    },
  },
  astria_structural: {
    build_seconds: Number(oursRun.seconds.toFixed(2)),
    nodes: oursGraph ? num(oursGraph[1]) : (stat(/^Nodes: (\d+)/m) ? num(stat(/^Nodes: (\d+)/m)) : null),
    edges: oursGraph ? num(oursGraph[2]) : (stat(/^Edges: (\d+)/m) ? num(stat(/^Edges: (\d+)/m)) : null),
    communities: stat(/^Communities: (\d+)/m) ? Number(stat(/^Communities: (\d+)/m)) : null,
    benchmark: {
      corpus_tokens: oursCorpusTokens ? num(oursCorpusTokens[1]) : null,
      avg_query_tokens: oursAvgQuery ? num(oursAvgQuery[1]) : null,
      reduction: oursReduction ? Number(oursReduction[1]) : null,
    },
  },
  methodology_notes: [
    'Structural pipeline only on both sides (no LLM enrichment): detect -> AST extract -> build -> cluster -> analyze -> report.',
    'The original is driven by scripts/bench/orig_run.py, replicating its own skill.md stage-for-stage.',
    'Each token benchmark is its own implementation; the ratio, not absolute tokens, is the comparable metric.',
    'Single cold run per tool on shared hardware - treat as trend data, not a microbenchmark.',
  ],
};

const outPath = path.join(repoRoot, 'website', 'src', 'data', 'benchmarks-snapshot.json');
mkdirSync(path.dirname(outPath), { recursive: true });
writeFileSync(outPath, JSON.stringify(snapshot, null, 2) + '\n');
console.log('snapshot written:', outPath);
console.log(JSON.stringify(snapshot, null, 2));
