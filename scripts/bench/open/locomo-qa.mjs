// LoCoMo QA-accuracy leg under Graphify's published protocol shape.
//
// Graphify's memory table (bench-work/corpus/BENCHMARKS.md) reports LOCOMO
// QA accuracy = key-fact coverage grading: an LLM reader answers from the
// system's retrieved context, a judge scores how many atomic key facts the
// answer covers (coverage = (covered + 0.5*partial) / total), and one shared
// model fills every LLM role. This adapter follows that shape with a
// disclosed difference: the reader and judge are gpt-4o-mini via OpenRouter
// (Graphify used Kimi K2.6), and the key-fact set is the LoCoMo reference
// answer plus its evidence lines rather than a precomputed atomic-fact set.
// Numbers are therefore directional, not cross-publishable without rerunning
// both systems under one shared model.
//
// Retrieval uses the astria graph over the transcript corpus (rebuilt fresh
// with the current binary); each query's NODE output is the reader's context.
//
// Usage: node scripts/bench/open/locomo-qa.mjs <n>
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const repo0 = path.resolve(import.meta.dirname, '..', '..', '..');
const work = path.join(repo0, 'bench-work', 'open');
const cli = path.join(repo0, 'bench-work', 'modes-20260928', 'cli', 'dist', 'index.js');
const corpus = path.join(repo0, 'bench-work', 'locomo-corpus');
const N = Number(process.argv[2] || 30);
const MODEL = 'openai/gpt-4o-mini';
const env = {
  ...process.env,
  NODE_PATH: path.join(repo0, 'node_modules'),
  ASTRIA_LLM_BACKEND: 'openai',
  ASTRIA_LLM_MODEL: MODEL,
  ASTRIA_LLM_BASE_URL: 'https://openrouter.ai/api/v1',
  ASTRIA_LLM_API_KEY: readFileSync(path.join(repo0, 'bench-work', 'llm-exp', 'or_key.txt'), 'utf8').trim(),
};

const qa = readFileSync(path.join(repo0, 'bench-work', 'locomo-qa.jsonl'), 'utf8')
  .trim().split('\n').map(JSON.parse)
  .filter(item => item.category !== 5) // adversarial: no key facts to cover
  .slice(0, N);

const run = (cmd, args, opts = {}) =>
  spawnSync(cmd, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env, ...opts });

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

const chat = async (messages) => {
  const res = await fetch('https://openrouter.ai/api/v1/chat/completions', {
    method: 'POST',
    headers: { Authorization: `Bearer ${env.ASTRIA_LLM_API_KEY}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ model: MODEL, messages, max_tokens: 500, temperature: 0 }),
  });
  const d = await res.json();
  if (!d.choices) throw Error(`LLM error: ${JSON.stringify(d).slice(0, 200)}`);
  return { text: d.choices[0].message.content, usage: d.usage };
};

// Fresh rebuild so the graph provably matches the current binary.
const build = run(process.execPath, [cli, 'run', corpus]);
if (build.status !== 0) throw Error(`graph build failed: ${build.stderr}`);

let spendIn = 0, spendOut = 0;
const rows = [];
for (const [i, item] of qa.entries()) {
  const q = run(process.execPath, [cli, 'query', item.question, '--budget', '4000', '--depth', '2'], { cwd: corpus });
  if (q.status !== 0) console.error(`query failed for ${item.id}: status=${q.status} err=${(q.stderr || q.error || '').slice(0, 200)}`);
  // Reader context = the sources retrieval surfaced (top 3 ranked files),
  // mirroring how a RAG harness feeds hits to the reader — the NODE text
  // itself is navigation, not evidence.
  const ranked = files(q.stdout || '', corpus).slice(0, 3);
  let context = '';
  for (const rel of ranked) {
    try {
      context += `
--- ${rel} ---
` + readFileSync(path.join(corpus, rel), 'utf8');
    } catch {}
    if (context.length > 12_000) break;
  }
  context = context.slice(0, 14_000);

  const answer = await chat([
    { role: 'system', content: 'Answer the question using only the provided graph context. Be concise and concrete. If the context does not contain the answer, say you cannot find it.' },
    { role: 'user', content: `Context:\n${context}\n\nQuestion: ${item.question}` },
  ]);
  spendIn += answer.usage?.prompt_tokens || 0;
  spendOut += answer.usage?.completion_tokens || 0;

  const judge = await chat([
    { role: 'system', content: 'You grade answers against a reference. List the distinct key facts asserted by the reference (1-4 facts), then for each fact output a line "FACT <n>: <covered|partial|missing>" plus a verbatim quote from the answer or "none". Then a final line "TOTAL: <covered> <partial> <missing>." Be strict: paraphrase counts as covered only if the fact is fully conveyed.' },
    { role: 'user', content: `Question: ${item.question}\nReference answer: ${item.answer}\nSystem answer: ${answer.text}` },
  ]);
  spendIn += judge.usage?.prompt_tokens || 0;
  spendOut += judge.usage?.completion_tokens || 0;

  // Judge writes either "TOTAL: 1 0 1" or "TOTAL: 1 covered 0 partial 1 missing."
  const m = judge.text.match(/TOTAL:\s*(\d+)\s*(?:covered)?\s+(\d+)\s*(?:partial)?\s+(\d+)\s*(?:missing)?/i);
  const [c, p, mi] = m ? [+m[1], +m[2], +m[3]] : [0, 0, 1];
  const coverage = c + p + mi > 0 ? (c + 0.5 * p) / (c + p + mi) : 0;
  rows.push({
    id: item.id, category: item.category,
    evidence_files: item.evidence_files,
    coverage: +coverage.toFixed(3),
    correct: coverage >= 0.5 ? 1 : 0,
    answer: answer.text.slice(0, 300),
    ranked,
    judge_raw: m ? undefined : judge.text.slice(0, 300),
  });
  console.log(`[${i + 1}/${qa.length}] ${item.id} coverage=${coverage.toFixed(2)}`);
}

const n = rows.length || 1;
const summary = {
  benchmark: 'LOCOMO QA accuracy (Graphify-protocol shape)',
  subset: `first ${rows.length} non-adversarial QA pairs (smoke)`,
  n: rows.length,
  qa_accuracy_at_least_half_coverage: +(rows.filter(r => r.correct).length / n).toFixed(3),
  mean_coverage: +(rows.reduce((s, r) => s + r.coverage, 0) / n).toFixed(3),
  provenance: {
    dataset: 'snap-research LoCoMo (via scripts/bench/memory/prepare-locomo.mjs)',
    reader: MODEL, judge: MODEL,
    differences_vs_graphify: ['model: gpt-4o-mini not Kimi K2.6', 'key facts from reference answer, not precomputed atomic set', 'single judge, no second-judge validation', 'smoke subset, not n=300'],
    spend: { input_tokens: spendIn, output_tokens: spendOut },
  },
};
writeFileSync(path.join(work, 'locomo-qa-results.json'), JSON.stringify({ summary, rows }, null, 2));
console.log(JSON.stringify(summary, null, 1));
