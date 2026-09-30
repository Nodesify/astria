// LoCoMo QA-accuracy leg under Graphify's published protocol shape, with a
// three-way decomposition of where accuracy is lost.
//
// Graphify's memory table (bench-work/corpus/BENCHMARKS.md) reports LOCOMO
// QA accuracy = key-fact coverage grading: an LLM reader answers from the
// system's retrieved context, a judge scores how many atomic key facts the
// answer covers (coverage = (covered + 0.5*partial) / total), and one shared
// model fills every LLM role. This adapter follows that shape with disclosed
// differences: the reader and judge are gpt-4o-mini via OpenRouter (Graphify
// used Kimi K2.6), and the key-fact set is the LoCoMo reference answer plus
// its evidence lines rather than a precomputed atomic-fact set.
//
// Decomposition (per tool, plus a tool-independent ceiling):
//   retrieval   did the graph surface the gold evidence sessions? (keyless)
//   retrieved   reader over the retrieved top-3 sessions (the published leg)
//   ceiling     reader over the GOLD sessions — same assembly, no retrieval.
//               The gap ceiling - retrieved is what better retrieval could
//               still win; a ceiling failure is reader/judge/protocol loss
//               that retrieval cannot fix.
//
// Shared-model protocol: `--tool both` runs astria and Graphify under the
// SAME reader and judge model in one process, so the two retrieved legs are
// directly comparable (the cross-publishable shape; single-model runs are
// directional only). Graphify builds from a sibling corpus whose transcripts
// are the same session files at top level (it does not read astria's
// .astria/transcripts sidecars).
//
// Usage: node scripts/bench/open/locomo-qa.mjs <n> [--tool astria|graphify|both]
//          [--no-rebuild] [--skip-ceiling] [--model <name>] [--out <path>]
//          [--astria <cli>] [--python <path>] [--graphify-source <dir>]
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync, cpSync } from 'node:fs';
import path from 'node:path';

const repo0 = path.resolve(import.meta.dirname, '..', '..', '..');
const work = path.join(repo0, 'bench-work', 'open');
const corpus = path.join(repo0, 'bench-work', 'locomo-corpus');

function parseArgs(argv) {
  const o = {
    n: Number(argv[2] && !argv[2].startsWith('--') ? argv[2] : 30),
    tool: 'astria',
    rebuild: true,
    ceiling: true,
    model: 'openai/gpt-4o-mini',
    out: path.join(work, 'locomo-qa-results.json'),
    astria: path.join(repo0, 'packages', 'astria-cli', 'dist', 'index.js'),
    python: path.join(repo0, 'bench-work', 'venv', 'Scripts', 'python.exe'),
    graphifySource: path.join(repo0, 'bench-work', 'corpus'),
  };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--tool') o.tool = argv[++i];
    else if (a === '--no-rebuild') o.rebuild = false;
    else if (a === '--skip-ceiling') o.ceiling = false;
    else if (a === '--model') o.model = argv[++i];
    else if (a === '--out') o.out = path.resolve(argv[++i]);
    else if (a === '--astria') o.astria = path.resolve(argv[++i]);
    else if (a === '--python') o.python = path.resolve(argv[++i]);
    else if (a === '--graphify-source') o.graphifySource = path.resolve(argv[++i]);
  }
  if (!['astria', 'graphify', 'both'].includes(o.tool)) throw Error(`--tool must be astria|graphify|both, got ${o.tool}`);
  return o;
}
const opts = parseArgs(process.argv);

const apiKey = process.env.OPENROUTER_API_KEY
  ?? (existsSync(path.join(repo0, 'bench-work', 'llm-exp', 'or_key.txt'))
      ? readFileSync(path.join(repo0, 'bench-work', 'llm-exp', 'or_key.txt'), 'utf8').trim()
      : null);
if (!apiKey) throw Error('need an OpenRouter key: set OPENROUTER_API_KEY or provide bench-work/llm-exp/or_key.txt');

const run = (cmd, args, o = {}) =>
  spawnSync(cmd, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, ...o });

const qa = readFileSync(path.join(repo0, 'bench-work', 'locomo-qa.jsonl'), 'utf8')
  .trim().split('\n').map(JSON.parse)
  .filter(item => item.category !== 5) // adversarial: no key facts to cover
  .slice(0, opts.n);

/// Ranked source files out of a tool's NODE output (astria and Graphify
/// share the `NODE ... src=<path> loc=...` line shape).
function files(text, cwd) {
  const out = [];
  for (const line of text.split('\n')) {
    if (!line.startsWith('NODE ')) continue;
    const m = line.match(/\bsrc=(.*?) (?:loc|community)=/);
    if (!m) continue;
    let f = m[1].replace(/:\d+(?::\d+)?$/, '').replaceAll('\\', '/');
    if (path.isAbsolute(f)) f = path.relative(cwd, f).replaceAll('\\', '/');
    f = f.replace(/^\.\//, '');
    if (f && !out.includes(f)) out.push(f);
  }
  return out;
}

const chat = async (messages) => {
  const res = await fetch('https://openrouter.ai/api/v1/chat/completions', {
    method: 'POST',
    headers: { Authorization: `Bearer ${apiKey}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ model: opts.model, messages, max_tokens: 500, temperature: 0 }),
  });
  const d = await res.json();
  if (!d.choices) throw Error(`LLM error: ${JSON.stringify(d).slice(0, 200)}`);
  return { text: d.choices[0].message.content, usage: d.usage };
};

/// Reader context = the given session files, mirroring how a RAG harness
/// feeds hits to the reader — the NODE text itself is navigation, not
/// evidence. Top 3 files, 12k chars, same assembly for every leg.
function contextFrom(filesRel, root) {
  let context = '';
  for (const rel of filesRel.slice(0, 3)) {
    try {
      context += `\n--- ${rel} ---\n` + readFileSync(path.join(root, rel), 'utf8');
    } catch {}
    if (context.length > 12_000) break;
  }
  return context.slice(0, 14_000);
}

const READER_PROMPT = 'Answer the question using only the provided graph context. Be concise and concrete. If the context does not contain the answer, say you cannot find it.';
const JUDGE_PROMPT = 'You grade answers against a reference. List the distinct key facts asserted by the reference (1-4 facts), then for each fact output a line "FACT <n>: <covered|partial|missing>" plus a verbatim quote from the answer or "none". Then a final line "TOTAL: <covered> <partial> <missing>." Be strict: paraphrase counts as covered only if the fact is fully conveyed.';

async function answerAndGrade(question, context, reference, spend) {
  const answer = await chat([
    { role: 'system', content: READER_PROMPT },
    { role: 'user', content: `Context:\n${context}\n\nQuestion: ${question}` },
  ]);
  spend.in += answer.usage?.prompt_tokens || 0;
  spend.out += answer.usage?.completion_tokens || 0;
  const judge = await chat([
    { role: 'system', content: JUDGE_PROMPT },
    { role: 'user', content: `Question: ${question}\nReference answer: ${reference}\nSystem answer: ${answer.text}` },
  ]);
  spend.in += judge.usage?.prompt_tokens || 0;
  spend.out += judge.usage?.completion_tokens || 0;
  const m = judge.text.match(/TOTAL:\s*(\d+)\s*(?:covered)?\s+(\d+)\s*(?:partial)?\s+(\d+)\s*(?:missing)?/i);
  const [c, p, mi] = m ? [+m[1], +m[2], +m[3]] : [0, 0, 1];
  const coverage = c + p + mi > 0 ? (c + 0.5 * p) / (c + p + mi) : 0;
  return { coverage, correct: coverage >= 0.5 ? 1 : 0, answer: answer.text.slice(0, 300) };
}

/// Gold session names are bare ("conv26-session01.md"); a tool's ranked
/// paths carry directories, so gold membership is a basename comparison.
const goldIn = (gold, ranked, k) => gold.some(g => ranked.slice(0, k).some(r => path.basename(r) === g));
const goldRank = (gold, ranked) => {
  let best = Infinity;
  for (const g of gold) {
    const idx = ranked.findIndex(r => path.basename(r) === g);
    if (idx !== -1) best = Math.min(best, idx + 1);
  }
  return best;
};

// ---- tool setup -----------------------------------------------------------

const tools = [];
if (opts.tool !== 'graphify') {
  const env = { ...process.env, NODE_PATH: path.join(repo0, 'node_modules') };
  if (opts.rebuild) {
    const build = run(process.execPath, [opts.astria, 'run', corpus], { env });
    if (build.status !== 0) throw Error(`astria graph build failed: ${build.stderr}`);
  }
  tools.push({ name: 'astria', root: corpus, query: q => run(process.execPath, [opts.astria, 'query', q, '--budget', '4000', '--depth', '2'], { cwd: corpus, env }) });
}
if (opts.tool !== 'astria') {
  // Graphify cannot see astria's .astria/transcripts sidecars; give it the
  // same session files as plain top-level markdown in a sibling corpus.
  const gcorpus = path.join(repo0, 'bench-work', 'locomo-corpus-graphify');
  mkdirSync(path.join(gcorpus, 'transcripts'), { recursive: true });
  cpSync(path.join(corpus, '.astria', 'transcripts'), path.join(gcorpus, 'transcripts'), { recursive: true });
  const imported = run(opts.python, ['-c', 'import graphify; print(graphify.__file__)']);
  if (imported.status !== 0 || !path.resolve(imported.stdout.trim()).startsWith(path.resolve(opts.graphifySource) + path.sep))
    throw Error(`python must import Graphify from the pinned source (${opts.graphifySource}); got: ${imported.stdout.trim() || imported.stderr}`);
  const build = run(opts.python, [path.join(repo0, 'scripts/bench/orig_run.py'), gcorpus, path.join(work, 'locomo-graphify-build.json'), '--include-documents'], { cwd: gcorpus });
  if (build.status !== 0) throw Error(`graphify build failed: ${build.stderr}`);
  const ggraph = path.join(gcorpus, 'graphify-out', 'graph.json');
  if (!existsSync(ggraph)) throw Error('graphify build produced no graph.json');
  tools.push({ name: 'graphify', root: gcorpus, query: q => run(opts.python, ['-m', 'graphify', 'query', q, '--budget', '4000', '--graph', ggraph], { cwd: gcorpus }) });
}

// ---- legs -----------------------------------------------------------------

const spend = { in: 0, out: 0 };
const rows = [];
const goldRows = [];

for (const [i, item] of qa.entries()) {
  const row = { id: item.id, category: item.category, evidence_files: item.evidence_files, tools: {} };
  for (const tool of tools) {
    const q = tool.query(item.question);
    if (q.status !== 0) console.error(`[${tool.name}] query failed for ${item.id}: status=${q.status} err=${(q.stderr || q.error || '').slice(0, 200)}`);
    const ranked = files(q.stdout || '', tool.root).slice(0, 3);
    const goldTop1 = goldIn(item.evidence_files, ranked, 1);
    const goldTop3 = goldIn(item.evidence_files, ranked, 3);
    const rank = goldRank(item.evidence_files, ranked);
    const leg = await answerAndGrade(item.question, contextFrom(ranked, tool.root), item.answer, spend);
    row.tools[tool.name] = { ranked, gold_top1: goldTop1, gold_top3: goldTop3, gold_rank: Number.isFinite(rank) ? rank : null, ...leg };
  }
  if (opts.ceiling) {
    const goldRel = item.evidence_files.map(f => path.join('.astria', 'transcripts', f));
    const ceiling = await answerAndGrade(item.question, contextFrom(goldRel, corpus), item.answer, spend);
    goldRows.push({ id: item.id, category: item.category, ...ceiling });
    row.ceiling = { coverage: ceiling.coverage, correct: ceiling.correct };
  }
  rows.push(row);
  const bits = tools.map(t => `${t.name} cov=${row.tools[t.name].coverage.toFixed(2)} gold@3=${row.tools[t.name].gold_top3 ? 1 : 0}`)
    .concat(opts.ceiling ? [`ceiling=${row.ceiling.coverage.toFixed(2)}`] : []);
  console.log(`[${i + 1}/${qa.length}] ${item.id} ${bits.join(' ')}`);
}

// ---- summary --------------------------------------------------------------

const byCategory = (legs) => {
  const cats = {};
  for (const r of legs) {
    cats[r.category] = cats[r.category] || { n: 0, correct: 0, coverage: 0 };
    cats[r.category].n++;
    cats[r.category].correct += r.correct ?? 0;
    cats[r.category].coverage += r.coverage ?? 0;
  }
  return Object.fromEntries(Object.entries(cats).map(([c, v]) => [c, {
    n: v.n,
    accuracy: +(v.correct / v.n).toFixed(3),
    mean_coverage: +(v.coverage / v.n).toFixed(3),
  }]));
};

const toolSummaries = {};
for (const tool of tools) {
  const legs = rows.map(r => ({ ...r.tools[tool.name], category: r.category }));
  const ceilings = goldRows.length
    ? rows.map(r => ({ correct: r.ceiling.correct, coverage: r.ceiling.coverage, category: r.category, gold_top3: r.tools[tool.name].gold_top3 }))
    : null;
  const n = legs.length || 1;
  const summary = {
    n: legs.length,
    retrieved: {
      qa_accuracy: +(legs.filter(l => l.correct).length / n).toFixed(3),
      mean_coverage: +(legs.reduce((s, l) => s + l.coverage, 0) / n).toFixed(3),
      by_category: byCategory(legs),
    },
    retrieval: {
      gold_top1: +(legs.filter(l => l.gold_top1).length / n).toFixed(3),
      gold_top3: +(legs.filter(l => l.gold_top3).length / n).toFixed(3),
      mean_gold_rank: +(legs.reduce((s, l) => s + (l.gold_rank ?? legs.length + 1), 0) / n).toFixed(2),
    },
  };
  if (ceilings) {
    const cn = ceilings.length || 1;
    summary.ceiling = {
      qa_accuracy: +(ceilings.filter(c => c.correct).length / cn).toFixed(3),
      mean_coverage: +(ceilings.reduce((s, c) => s + c.coverage, 0) / cn).toFixed(3),
    };
    // Failure decomposition: every question that failed the retrieved leg.
    const failed = legs.map((l, i) => ({ l, c: ceilings[i] })).filter(x => !x.l.correct);
    summary.decomposition = {
      failures: failed.length,
      retrieval_miss: failed.filter(x => !x.l.gold_top3).length,
      gold_retrieved_but_lost: failed.filter(x => x.l.gold_top3).length,
      // of which the reader could answer from gold alone (assembly diluted
      // it) vs could not even with gold (reader/judge limit):
      lost_despite_gold_ceiling: failed.filter(x => x.l.gold_top3 && x.c.correct).length,
      ceiling_also_failed: failed.filter(x => x.l.gold_top3 && !x.c.correct).length,
      note: 'retrieval_miss counts questions whose top-3 held no gold session; ceiling numbers are tool-independent (same reader, gold context).',
    };
  }
  toolSummaries[tool.name] = summary;
}

const payload = {
  summary: {
    benchmark: 'LOCOMO QA accuracy (Graphify-protocol shape) + retrieval/reader decomposition',
    subset: `first ${qa.length} non-adversarial QA pairs`,
    n: qa.length,
    shared_model: opts.model,
    tools: toolSummaries,
    provenance: {
      dataset: 'snap-research LoCoMo (via scripts/bench/memory/prepare-locomo.mjs)',
      reader: opts.model, judge: opts.model,
      reader_context: 'top-3 retrieved session transcripts, 12k-char assembly (identical across legs and tools)',
      differences_vs_graphify_published: [
        'shared reader/judge model for every tool in this run',
        'key facts from reference answer, not precomputed atomic set',
        'single judge, no second-judge validation',
      ],
      graphify: opts.tool !== 'astria' ? { source: opts.graphifySource, python: opts.python } : undefined,
      spend: { input_tokens: spend.in, output_tokens: spend.out },
    },
  },
  rows,
  ceiling_rows: goldRows,
};
mkdirSync(path.dirname(opts.out), { recursive: true });
writeFileSync(opts.out, JSON.stringify(payload, null, 2));
console.log(JSON.stringify(payload.summary, null, 1));
console.log(`results: ${opts.out}`);
