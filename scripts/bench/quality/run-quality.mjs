#!/usr/bin/env node
// Retrieval-quality benchmark for astria: runs a golden QA set through the
// real query engine and measures recall@k and MRR of the files and symbols
// the answers surface. Token cost is measured separately (benchmark.rs and
// the snapshot runner); this measures whether the graph answers WELL.
//
// Zero runtime dependencies.
//
// Usage:
//   node scripts/bench/quality/run-quality.mjs [--root <dir>] [--golden <jsonl>]
//        [--astria "<cmd prefix>"] [--budget 4000] [--depth 3] [--k 1,3,5,10]
//        [--out <path>] [--check]
//
//   --check  validate the golden set only: schema, and that every
//            expected_files entry matches a real file under --root.

import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, '..', '..', '..');

function parseArgs(argv) {
  const opts = {
    root: repoRoot,
    golden: path.join(scriptDir, 'golden', 'astria-self.jsonl'),
    budget: '4000',
    depth: '3',
    k: [1, 3, 5, 10],
    out: path.join(scriptDir, 'out', 'quality-results.json'),
    check: false,
    astria: process.env.ASTRIA_BIN || 'astria',
    embed: false,
    min_recall5: 0,
  };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--root') opts.root = path.resolve(argv[++i]);
    else if (a === '--golden') opts.golden = path.resolve(argv[++i]);
    else if (a === '--astria') opts.astria = argv[++i];
    else if (a === '--budget') opts.budget = argv[++i];
    else if (a === '--depth') opts.depth = argv[++i];
    else if (a === '--k') opts.k = argv[++i].split(',').map(Number);
    else if (a === '--out') opts.out = path.resolve(argv[++i]);
    else if (a === '--min-recall5') opts.min_recall5 = Number(argv[++i]);
    else if (a === '--check') opts.check = true;
    else { console.error(`unknown arg: ${a}`); process.exit(2); }
  }
  return opts;
}

const norm = (p) => String(p).replace(/\\/g, '/').toLowerCase();

function walkFiles(root) {
  const skip = new Set(['.git', '.astria', '.graphify', 'node_modules', 'target', 'bench-work', 'dist']);
  const out = [];
  const visit = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.name.startsWith('.')) continue;
      if (skip.has(entry.name)) continue;
      const p = path.join(dir, entry.name);
      if (entry.isDirectory()) visit(p);
      else out.push(norm(path.relative(root, p)));
    }
  };
  visit(root);
  return out;
}

function loadGolden(file) {
  const items = [];
  const lines = readFileSync(file, 'utf8').split('\n').filter((l) => l.trim());
  for (const [i, line] of lines.entries()) {
    let o;
    try { o = JSON.parse(line); } catch (e) { throw new Error(`${file}:${i + 1} invalid JSON: ${e.message}`); }
    if (typeof o.id !== 'string' || typeof o.question !== 'string') {
      throw new Error(`${file}:${i + 1} needs string "id" and "question"`);
    }
    if (!Array.isArray(o.expected_files) || o.expected_files.length === 0) {
      throw new Error(`${file}:${i + 1} needs non-empty "expected_files"`);
    }
    items.push({
      id: o.id,
      question: o.question,
      expected_files: o.expected_files.map(norm),
      expected_symbols: (o.expected_symbols ?? []).map((s) => String(s).toLowerCase()),
    });
  }
  return items;
}

/// Parse the query engine's text output into a ranked, deduped list of source
/// files and node names. NODE lines look like
/// `NODE <label> [id=<id> src=<path[:line]> community=<n>]`,
/// EDGE lines carry `@<path:line>` when the edge has a source location.
function parseAnswer(text) {
  const files = [];
  const names = [];
  const seenFile = new Set();
  const seenName = new Set();
  const pushFile = (p) => {
    const clean = norm(p).replace(/:\d+$/, '');
    if (clean && !seenFile.has(clean)) { seenFile.add(clean); files.push(clean); }
  };
  const pushName = (n) => {
    const clean = n.trim().toLowerCase();
    if (clean && !seenName.has(clean)) { seenName.add(clean); names.push(clean); }
  };
  for (const m of text.matchAll(/^NODE (.+?) \[id=(.*?) src=(\S+?) community=/gm)) {
    pushName(m[1]); pushName(m[2]); pushFile(m[3]);
  }
  for (const m of text.matchAll(/^EDGE .*? @(\S+?):\d+$/gm)) pushFile(m[1]);
  return { files, names };
}

/// Rank (1-based) of the best hit, or Infinity when nothing matched.
function bestRank(expected, ranked) {
  let best = Infinity;
  for (const e of expected) {
    const idx = ranked.findIndex((r) => r.includes(e));
    if (idx !== -1) best = Math.min(best, idx + 1);
  }
  return best;
}

async function main() {
  const opts = parseArgs(process.argv);
  const items = loadGolden(opts.golden);

  // Validate expectations against the real tree — golden paths must exist.
  const treeFiles = walkFiles(opts.root);
  const bad = [];
  for (const item of items) {
    for (const e of item.expected_files) {
      if (!treeFiles.some((f) => f.includes(e))) bad.push(`${item.id}: no file matches "${e}"`);
    }
  }
  if (bad.length) {
    console.error(`golden set has ${bad.length} ungrounded expectation(s):`);
    for (const b of bad) console.error('  ' + b);
    process.exit(1);
  }
  console.log(`golden set ok: ${items.length} questions, all expectations grounded`);
  if (opts.check) return;

  const cli = opts.astria;
  const results = [];
  for (const item of items) {
    const cmd = `${cli} query "${item.question}" --budget ${opts.budget} --depth ${opts.depth}`;
    const t0 = Date.now();
    const r = spawnSync(cmd, { cwd: opts.root, encoding: 'utf8', shell: true, timeout: 120_000, maxBuffer: 32 * 1024 * 1024 });
    const seconds = (Date.now() - t0) / 1000;
    if (!r.stdout) {
      results.push({ ...item, error: (r.stderr || String(r.error)).slice(0, 300), seconds: Number(seconds.toFixed(2)) });
      console.error(`  ${item.id} ERROR ${seconds.toFixed(1)}s`);
      continue;
    }
    const { files: rankedFiles, names: rankedNames } = parseAnswer(r.stdout);
    const fileRank = bestRank(item.expected_files, rankedFiles);
    const symRank = item.expected_symbols.length ? bestRank(item.expected_symbols, rankedNames) : Infinity;
    const hit = Math.min(fileRank, symRank);
    results.push({
      id: item.id,
      question: item.question,
      seconds: Number(seconds.toFixed(2)),
      hit_rank: Number.isFinite(hit) ? hit : null,
      matched_file: Number.isFinite(fileRank) ? rankedFiles[fileRank - 1] : null,
      matched_symbol: Number.isFinite(symRank) ? rankedNames[symRank - 1] : null,
    });
    console.log(`  ${item.id} rank=${Number.isFinite(hit) ? hit : 'miss'} ${seconds.toFixed(1)}s`);
  }

  const ok = results.filter((r) => !r.error);
  const summary = { questions: results.length, answered: ok.length, mrr: 0 };
  for (const k of opts.k) summary[`recall@${k}`] = 0;
  for (const r of ok) {
    for (const k of opts.k) if (r.hit_rank !== null && r.hit_rank <= k) summary[`recall@${k}`] += 1;
    summary.mrr += r.hit_rank !== null ? 1 / r.hit_rank : 0;
  }
  const n = ok.length || 1;
  for (const k of opts.k) summary[`recall@${k}`] = Number((summary[`recall@${k}`] / n).toFixed(4));
  summary.mrr = Number((summary.mrr / n).toFixed(4));
  summary.avg_query_seconds = Number((ok.reduce((s, r) => s + r.seconds, 0) / n).toFixed(2));

  const payload = {
    generated_at: new Date().toISOString(),
    corpus_root: norm(opts.root),
    golden_set: path.basename(opts.golden),
    cli,
    query_budget: Number(opts.budget),
    query_depth: Number(opts.depth),
    summary,
    items: results,
  };
  mkdirSync(path.dirname(opts.out), { recursive: true });
  writeFileSync(opts.out, JSON.stringify(payload, null, 2) + '\n');
  console.log(`\nsummary: ${JSON.stringify(summary)}`);
  console.log(`results: ${opts.out}`);

  // Blocking gate: a recall@5 floor lets CI fail on ranking regressions
  // without hard-coding a ceiling on quality.
  if (opts.min_recall5 > 0) {
    const r5 = summary['recall@5'] ?? 0;
    if (r5 < opts.min_recall5 / 100) {
      console.error(`recall@5 ${r5} is below the required ${(opts.min_recall5 / 100).toFixed(2)}`);
      process.exitCode = 1;
    } else {
      console.log(`recall@5 gate passed (${opts.min_recall5}%)`);
    }
  }
}

main().catch((e) => { console.error(e.message); process.exit(1); });
