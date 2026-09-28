// HotpotQA distractor retrieval adapter.
//
// Protocol: HotpotQA (CC BY-SA 4.0) validation split, distractor setting —
// each question ships 10 Wikipedia paragraphs of which exactly 2 are the
// gold supporting documents. Standard IR practice on this dataset grades
// supporting-document retrieval; answering (multi-hop QA) needs an LLM
// reader+judge and is a separate leg (see README).
//
// This adapter: fetch n questions, materialize each question's 10 paragraphs
// as namespaced markdown files (qNNN--Title.md so identical titles across
// questions never collide), build ONE astria graph over the whole corpus,
// query per question, and grade file-level hit@k / MRR against the
// namespaced gold files.
//
// Usage: node scripts/bench/open/hotpotqa.mjs <n>
import { spawnSync } from 'node:child_process';
import { writeFileSync, rmSync, mkdirSync } from 'node:fs';
import path from 'node:path';

const repo0 = path.resolve(import.meta.dirname, '..', '..', '..');
const work = path.join(repo0, 'bench-work', 'open');
const cli = path.join(repo0, 'bench-work', 'modes-20260928', 'cli', 'dist', 'index.js');
const N = Number(process.argv[2] || 100);
const env = { ...process.env, NODE_PATH: path.join(repo0, 'node_modules') };
const run = (cmd, args, opts = {}) =>
  spawnSync(cmd, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, env, ...opts });

const rowsUrl = (offset, length) =>
  `https://datasets-server.huggingface.co/rows?dataset=hotpotqa%2Fhotpot_qa&config=distractor&split=validation&offset=${offset}&length=${length}`;
const questions = [];
for (let offset = 0; offset < N; offset += 50) {
  const res = await fetch(rowsUrl(offset, Math.min(50, N - offset)));
  const d = await res.json();
  questions.push(...d.rows.map(r => r.row));
}
console.log(`fetched ${questions.length} questions`);

// Materialize the corpus: per-question namespaces keep gold targets distinct.
// Fresh directory per run: a locked .astria db (AV or a lingering process)
// must not wedge the whole adapter.
const corpusDir = path.join(work, `hotpotqa-corpus-${Date.now()}`);
mkdirSync(corpusDir, { recursive: true });
const gold = [];
questions.forEach((q, qi) => {
  const ns = String(qi).padStart(3, '0');
  const titles = q.context.title;
  titles.forEach((title, ti) => {
    const body = q.context.sentences[ti].join('');
    const safe = title.replaceAll(/[<>:"/\\|?*]/g, '_');
    writeFileSync(path.join(corpusDir, `q${ns}--${safe}.md`), `# ${title}\n\n${body}\n`);
  });
  gold.push({ id: q.id, ns, gold: q.supporting_facts.title.map(t => `q${ns}--${t.replaceAll(/[<>:"/\\|?*]/g, '_')}.md`) });
});

const build = run(process.execPath, [cli, 'run', corpusDir]);
if (build.status !== 0) throw Error(`build failed: ${build.stderr}`);

const files = (text) => {
  const out = [];
  for (const line of text.split('\n')) {
    if (!line.startsWith('NODE ')) continue;
    const m = line.match(/\bsrc=(.*?) (?:loc|community)=/);
    if (!m) continue;
    let f = m[1].replace(/:\d+(?::\d+)?$/, '').replaceAll('\\', '/');
    if (path.isAbsolute(f)) f = path.relative(corpusDir, f).replaceAll('\\', '/');
    if (f && !out.includes(f)) out.push(f);
  }
  return out;
};
const labels = (text) => [...text.matchAll(/^NODE (.+?) \[id=/gm)].map(m => m[1]);

const rows = [];
for (const [i, item] of gold.entries()) {
  const q = questions[i];
  const runQuery = (query, budget) =>
    run(process.execPath, [cli, 'query', query, '--budget', String(budget), '--depth', '2'], { cwd: corpusDir });

  const single = runQuery(q.question, 2000);
  const singleRanked = single.status === 0 ? files(single.stdout) : [];

  // Two-stage bridge retrieval: stage 1 stands alone; stage 2 re-queries
  // seeded with stage-1's top labels. Naive two-stage biases toward the
  // first hop, so the primary variant is a rank merge: stage-1 order first,
  // then stage-2-only discoveries appended.
  const bridgeLabels = single.status === 0 ? [...new Set(labels(single.stdout))].slice(0, 3) : [];
  const twoStage = bridgeLabels.length
    ? runQuery(`${q.question} ${bridgeLabels.join(' ')}`, 2000)
    : single;
  const twoRanked = twoStage.status === 0 ? files(twoStage.stdout) : [];
  const merged = [...singleRanked, ...twoRanked.filter(f => !singleRanked.includes(f))];

  const rankOf = (ranked) => {
    const ranks = item.gold.map(g => ranked.indexOf(g) + 1).filter(x => x > 0);
    return ranks.length ? Math.min(...ranks) : null;
  };
  const metrics = (ranked) => ({
    rank_best: rankOf(ranked),
    both_in_5: item.gold.every(g => ranked.slice(0, 5).includes(g)) ? 1 : 0,
    any_hit: ranked.some(g => item.gold.includes(g)),
  });
  rows.push({
    id: item.id,
    single: metrics(singleRanked),
    twostage: metrics(twoRanked),
    merged: metrics(merged),
    bridge_labels: bridgeLabels,
  });
  if ((i + 1) % 20 === 0) console.log(`[${i + 1}/${gold.length}]`);
}

const summarize = (key) => {
  const n = rows.length || 1;
  return {
    n: rows.length,
    mrr: +(rows.reduce((s, r) => s + (r[key].rank_best ? 1 / r[key].rank_best : 0), 0) / n).toFixed(3),
    hit1: +(rows.filter(r => r[key].rank_best === 1).length / n).toFixed(3),
    hit5: +(rows.filter(r => r[key].rank_best && r[key].rank_best <= 5).length / n).toFixed(3),
    both_gold_in_5: +(rows.filter(r => r[key].both_in_5).length / n).toFixed(3),
    hit_any: +(rows.filter(r => r[key].any_hit).length / n).toFixed(3),
  };
};
const summary = {
  benchmark: 'HotpotQA distractor supporting-document retrieval',
  subset: `first ${rows.length} validation questions (smoke)`,
  single: summarize('single'),
  twostage: summarize('twostage'),
  merged: summarize('merged'),
  budget: 2000, depth: 2,
  provenance: { dataset: 'hotpotqa/hotpot_qa validation distractor', license: 'CC BY-SA 4.0', corpus: 'per-question 10 paragraphs, namespaced qNNN--Title.md, one graph', note: 'twostage = question + top-3 stage-1 NODE labels re-queried; retrieval leg only' },
};
writeFileSync(path.join(work, 'hotpotqa-results.json'), JSON.stringify({ summary, rows }, null, 2));
console.log(JSON.stringify(summary, null, 1));
