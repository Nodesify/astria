#!/usr/bin/env node
// Score astria against the LoCoMo memory benchmark: build the graph over the
// prepared transcript corpus (prepare-locomo.mjs), answer each QA question
// with the real query engine, and measure recall@k / MRR of the evidence
// session files. Optional LLM-judged answer correctness with --judge.
//
//   node scripts/bench/memory/run-locomo.mjs \
//     [--corpus bench-work/locomo-corpus] [--qa bench-work/locomo-qa.jsonl] \
//     [--out bench-work/locomo-results.json] [--limit N] [--k 1,3,5,10]
//     [--no-build] [--judge]
//
// --judge needs ANTHROPIC_API_KEY (claude-haiku grading) and adds
// "judged_correct" per row; without it the run is recall-only.

import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '..', '..', '..');

function parseArgs(argv) {
  const o = {
    corpus: path.join(repoRoot, 'bench-work', 'locomo-corpus'),
    qa: path.join(repoRoot, 'bench-work', 'locomo-qa.jsonl'),
    out: path.join(repoRoot, 'bench-work', 'locomo-results.json'),
    k: [1, 3, 5, 10],
    limit: 0,
    build: true,
    judge: false,
  };
  for (let i = 2; i < argv.length; i++) {
    if (argv[i] === '--corpus') o.corpus = path.resolve(argv[++i]);
    else if (argv[i] === '--qa') o.qa = path.resolve(argv[++i]);
    else if (argv[i] === '--out') o.out = path.resolve(argv[++i]);
    else if (argv[i] === '--limit') o.limit = Number(argv[++i]);
    else if (argv[i] === '--k') o.k = argv[++i].split(',').map(Number);
    else if (argv[i] === '--no-build') o.build = false;
    else if (argv[i] === '--judge') o.judge = true;
    else { console.error(`unknown arg: ${argv[i]}`); process.exit(2); }
  }
  return o;
}

/// Ranked, deduped source files surfaced by a query answer (NODE `src=`).
function parseAnswer(text) {
  const files = [];
  const seen = new Set();
  for (const m of text.matchAll(/^NODE .+? \[id=.*? src=(\S+?) community=/gm)) {
    const f = m[1].replace(/:\d+$/, '').toLowerCase();
    if (f && !seen.has(f)) { seen.add(f); files.push(f); }
  }
  return files;
}

function bestRank(expected, ranked) {
  let best = Infinity;
  for (const e of expected) {
    const idx = ranked.findIndex((f) => f.includes(e));
    if (idx !== -1) best = Math.min(best, idx + 1);
  }
  return best;
}

async function judgeAnswer(question, answer, gold) {
  const key = process.env.ANTHROPIC_API_KEY;
  if (!key) throw new Error('--judge needs ANTHROPIC_API_KEY');
  const body = JSON.stringify({
    model: 'claude-haiku-4-5',
    max_tokens: 16,
    messages: [{
      role: 'user',
      content:
        `Question: ${question}\n\nReference answer: ${gold}\n\n` +
        `System answer: ${answer}\n\nDoes the system answer convey the same ` +
        `fact as the reference answer? Reply with exactly CORRECT, PARTIAL or WRONG.`,
    }],
  });
  const res = await fetch('https://api.anthropic.com/v1/messages', {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'x-api-key': key,
      'anthropic-version': '2023-06-01',
    },
    body,
  });
  if (!res.ok) throw new Error(`judge api ${res.status}`);
  const data = await res.json();
  const text = (data.content?.[0]?.text ?? '').trim().toUpperCase();
  return text.startsWith('CORRECT') ? 'CORRECT'
    : text.startsWith('PARTIAL') ? 'PARTIAL' : 'WRONG';
}

async function main() {
  const o = parseArgs(process.argv);
  const rows = readFileSync(o.qa, 'utf8').split('\n').filter((l) => l.trim()).map(JSON.parse);
  const withEvidence = rows.filter((r) => r.evidence_files.length > 0);
  const items = (o.limit ? withEvidence.slice(0, o.limit) : withEvidence);
  console.log(`${withEvidence.length} evidence-backed QA pairs` +
    (o.limit ? ` (running first ${items.length})` : ''));

  if (o.build) {
    console.log('building graph ...');
    const r = spawnSync('astria run .', { cwd: o.corpus, encoding: 'utf8', shell: true, timeout: 600_000 });
    if (r.status !== 0) { console.error(r.stderr || r.stdout); process.exit(1); }
  }

  const results = [];
  for (const item of items) {
    const r = spawnSync(`astria query "${item.question}" --budget 4000`, {
      cwd: o.corpus, encoding: 'utf8', shell: true, timeout: 120_000, maxBuffer: 32 * 1024 * 1024,
    });
    if (!r.stdout) { results.push({ ...item, error: String(r.stderr || r.error).slice(0, 300) }); continue; }
    const ranked = parseAnswer(r.stdout);
    const rank = bestRank(item.evidence_files, ranked);
    const row = {
      ...item,
      hit_rank: Number.isFinite(rank) ? rank : null,
      surfaced_top: ranked.slice(0, 3),
    };
    if (o.judge) row.judged = await judgeAnswer(item.question, r.stdout, item.answer);
    results.push(row);
    console.log(`  ${item.id} rank=${Number.isFinite(rank) ? rank : 'miss'}${row.judged ? ' judge=' + row.judged : ''}`);
  }

  const ok = results.filter((r) => !r.error);
  const summary = { questions: ok.length, mrr: 0 };
  for (const k of o.k) summary[`recall@${k}`] = 0;
  for (const r of ok) {
    for (const k of o.k) if (r.hit_rank !== null && r.hit_rank <= k) summary[`recall@${k}`] += 1;
    summary.mrr += r.hit_rank !== null ? 1 / r.hit_rank : 0;
  }
  const n = ok.length || 1;
  for (const k of o.k) summary[`recall@${k}`] = Number((summary[`recall@${k}`] / n).toFixed(4));
  summary.mrr = Number((summary.mrr / n).toFixed(4));
  if (o.judge) {
    const judged = ok.filter((r) => r.judged);
    summary.judged_correct = Number(
      (judged.filter((r) => r.judged === 'CORRECT').length / (judged.length || 1)).toFixed(4),
    );
  }

  mkdirSync(path.dirname(o.out), { recursive: true });
  writeFileSync(o.out, JSON.stringify({
    generated_at: new Date().toISOString(),
    benchmark: 'locomo',
    corpus: o.corpus,
    summary,
    items: results,
  }, null, 2) + '\n');
  console.log(`\nsummary: ${JSON.stringify(summary)}`);
  console.log(`results: ${o.out}`);
}

main().catch((e) => { console.error(e.message); process.exit(1); });
