#!/usr/bin/env node
// Blind answer-correctness judging: both tools answer the same golden
// questions on the same corpus, and a TypeSafe System One judge grades each
// answer against the golden rubric WITHOUT knowing which tool produced it.
//
//   TYPESAFE_API_KEY=... node scripts/bench/quality/blind-judge.mjs \
//     [--golden scripts/bench/quality/golden/astria-self.jsonl] \
//     [--astria-corpus bench-work/self-check/corpus] \
//     [--astria-cli bench-work/self-runtime/dist/index.js] \
//     [--graphify-corpus bench-work/paired-1.0.6-20260927/astria-self-graphify] \
//     [--out scripts/bench/quality/out/blind-judge-results.json] [--limit N]
//
// The judge is a Score question over three levels (FAIL / PARTIAL / PASS);
// the verdict is the highest-probability level. Answers are truncated before
// judging; the rubric carries the ground truth (files/symbols), the judge
// never sees tool names.

import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');

function parseArgs(argv) {
  const o = {
    golden: path.join(repo, 'scripts', 'bench', 'quality', 'golden', 'astria-self.jsonl'),
    astriaCorpus: path.join(repo, 'bench-work', 'self-check', 'corpus'),
    astriaCli: path.join(repo, 'bench-work', 'self-runtime', 'dist', 'index.js'),
    graphifyCorpus: path.join(
      repo,
      'bench-work',
      'paired-1.0.6-20260927',
      'astria-self-graphify',
    ),
    graphifyGraph: 'graphify-out/graph.json',
    out: path.join(repo, 'scripts', 'bench', 'quality', 'out', 'blind-judge-results.json'),
    limit: 0,
  };
  for (let i = 2; i < argv.length; i++) {
    if (argv[i] === '--golden') o.golden = path.resolve(argv[++i]);
    else if (argv[i] === '--astria-corpus') o.astriaCorpus = path.resolve(argv[++i]);
    else if (argv[i] === '--astria-cli') o.astriaCli = path.resolve(argv[++i]);
    else if (argv[i] === '--graphify-corpus') o.graphifyCorpus = path.resolve(argv[++i]);
    else if (argv[i] === '--graphify-graph') o.graphifyGraph = argv[++i];
    else if (argv[i] === '--out') o.out = path.resolve(argv[++i]);
    else if (argv[i] === '--limit') o.limit = Number(argv[++i]);
    else {
      console.error(`unknown arg: ${argv[i]}`);
      process.exit(2);
    }
  }
  return o;
}

const ANSWER_LIMIT = 12_000;
const LEVELS = [
  'FAIL - topically related only, or points at documentation rather than the implementing code',
  'PARTIAL - narrows to the right area but does not identify the implementation',
  'PASS - names the file/symbol, or describes exactly what it does such that a developer could navigate there',
];

function rubricFor(g) {
  return (
    `Ground truth: this is implemented in ${g.expected_files.join(', ')}` +
    (g.expected_symbols?.length ? ` (${g.expected_symbols.join(', ')})` : '') +
    `. Question asked: "${g.question}". Judge whether the response identifies ` +
    `that implementation - naming the file/symbol, or describing exactly ` +
    `what it does such that a developer could navigate there. A response ` +
    `that is merely topically related, or points at documentation rather ` +
    `than the implementing code, is a FAIL.`
  );
}

function runAstria(o, question) {
  const r = spawnSync(
    process.execPath,
    [o.astriaCli, 'query', question, '--budget', '4000'],
    {
      cwd: o.astriaCorpus,
      encoding: 'utf8',
      timeout: 120_000,
      maxBuffer: 32 * 1024 * 1024,
      env: { ...process.env, NODE_PATH: path.join(repo, 'packages', 'astria-cli', 'node_modules') },
    },
  );
  return r.status === 0 ? r.stdout || '' : `ERROR: ${r.stderr || String(r.error)}`;
}

function runGraphify(o, question) {
  const r = spawnSync(
    path.join(repo, 'bench-work', 'venv', 'Scripts', 'python.exe'),
    [
      '-m',
      'graphify',
      'query',
      question,
      '--budget',
      '4000',
      '--graph',
      path.join(o.graphifyCorpus, o.graphifyGraph),
    ],
    {
      cwd: o.graphifyCorpus,
      encoding: 'utf8',
      timeout: 300_000,
      maxBuffer: 32 * 1024 * 1024,
      env: { ...process.env, PYTHONUTF8: '1' },
    },
  );
  return r.status === 0 ? r.stdout || '' : `ERROR: ${r.stderr || String(r.error)}`;
}

const sleep = (ms) => new Promise((res) => setTimeout(res, ms));

async function judge(key, model, question, rubric, answer) {
  const body = {
    model,
    state: {
      page: {
        url: 'bench://retrieval-answer',
        title: question,
        text:
          `Question: ${question}\n\n` +
          `Ground truth: ${rubric}\n\n` +
          `Answer to judge:\n${answer.slice(0, ANSWER_LIMIT)}`,
      },
    },
    questions: {
      verdict: {
        type: 'score',
        instructions:
          'How well does this answer identify the implementation described in the ground truth? ' +
          'Base the verdict only on the answer text; it is a raw retrieval dump, so file paths ' +
          'and symbol names count as identification.',
        criteria: LEVELS,
      },
    },
  };
  for (let attempt = 0; attempt < 3; attempt++) {
    let res;
    try {
      res = await fetch('https://api.typesafe.ai/v1/systemone', {
        method: 'POST',
        headers: { 'content-type': 'application/json', authorization: `Bearer ${key}` },
        body: JSON.stringify(body),
      });
    } catch {
      if (attempt === 2) throw new Error('judge connection failed');
      await sleep(500 * 2 ** attempt);
      continue;
    }
    if ([429, 529, 503].includes(res.status) && attempt < 2) {
      await sleep(500 * 2 ** attempt);
      continue;
    }
    if (!res.ok) throw new Error(`judge api ${res.status}`);
    const data = await res.json();
    const a = data.answers?.verdict ?? {};
    const probs = a.probabilities ?? {};
    const best = Object.entries(probs).sort((x, y) => y[1] - x[1])[0];
    const idx = best ? Number(best[0]) : a.score ? a.score - 1 : 0;
    return {
      level: LEVELS[idx]?.split(' - ')[0] ?? 'UNKNOWN',
      score: a.score ?? null,
      confidence: a.confidence ?? null,
      probabilities: probs,
    };
  }
  throw new Error('judge unavailable');
}

async function main() {
  const o = parseArgs(process.argv);
  const key = process.env.TYPESAFE_API_KEY;
  if (!key) {
    console.error('TYPESAFE_API_KEY is required');
    process.exit(1);
  }
  const model = process.env.TYPESAFE_MODEL || 'jev-latest';
  const golden = readFileSync(o.golden, 'utf8')
    .split('\n')
    .filter((l) => l.trim())
    .map(JSON.parse);
  const items = o.limit ? golden.slice(0, o.limit) : golden;
  console.log(`${items.length} questions; judging answers from both tools blind`);

  const rows = [];
  for (const [qi, g] of items.entries()) {
    const rubric = rubricFor(g);
    const answers = { astria: runAstria(o, g.question), graphify: runGraphify(o, g.question) };
    for (const tool of ['astria', 'graphify']) {
      let verdict;
      try {
        verdict = await judge(key, model, g.question, rubric, answers[tool]);
      } catch (e) {
        verdict = { level: 'ERROR', error: String(e.message ?? e) };
      }
      rows.push({
        id: g.id,
        question: g.question,
        tool,
        ...verdict,
        answer_chars: answers[tool].length,
      });
      console.log(`  ${g.id} ${tool}: ${verdict.level}${verdict.error ? ` (${verdict.error})` : ''}`);
    }
  }

  const summary = {};
  for (const tool of ['astria', 'graphify']) {
    const rs = rows.filter((r) => r.tool === tool && r.level !== 'ERROR');
    const n = rs.length || 1;
    summary[tool] = {
      judged: rs.length,
      pass: +(rs.filter((r) => r.level === 'PASS').length / n).toFixed(3),
      partial: +(rs.filter((r) => r.level === 'PARTIAL').length / n).toFixed(3),
      fail: +(rs.filter((r) => r.level === 'FAIL').length / n).toFixed(3),
      mean_score: +(rs.reduce((s, r) => s + (r.score ?? 0), 0) / n).toFixed(3),
      mean_confidence: +(rs.reduce((s, r) => s + (r.confidence ?? 0), 0) / n).toFixed(3),
    };
  }
  mkdirSync(path.dirname(o.out), { recursive: true });
  writeFileSync(
    o.out,
    JSON.stringify({ generated_at: new Date().toISOString(), judge: model, summary, rows }, null, 2) +
      '\n',
  );
  console.log(`summary: ${JSON.stringify(summary)}`);
  console.log(`out: ${o.out}`);
}

main().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
