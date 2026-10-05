#!/usr/bin/env node
// Retrieval-quality benchmark for astria: runs a golden QA set through the
// real query engine and measures exact-file hit@k, recall@k and MRR.
// File and source-grounded declaration metrics are separate from delivered tokens.
// Symbol-label matches remain diagnostics only.
//
// Exact token counts and the optional lexical baseline require js-tiktoken.
//
// Usage:
//   node scripts/bench/quality/run-quality.mjs [--root <dir>] [--golden <jsonl>]
//        [--astria <executable-or-js-entrypoint>] [--budget 4000] [--depth 3] [--k 1,3,5,10]
//        [--out <path>] [--check] [--baseline | --iterative-baseline] [--allow-reserved]
//
//   --check  validate the golden set only: schema, and that every
//            expected_files entry matches a real file under --root.

import { createHash } from 'node:crypto';
import { lexicalBaseline } from './lexical-baseline.mjs';
import { loadTokenizer } from '../tokenize.mjs';
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
    baseline: false,
    iterative: false,
    allowReserved: false,
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
    else if (a === '--baseline') opts.baseline = true;
    else if (a === '--iterative-baseline') { opts.baseline = true; opts.iterative = true; }
    else if (a === '--allow-reserved') opts.allowReserved = true;
    else if (a === '--check') opts.check = true;
    else { console.error(`unknown arg: ${a}`); process.exit(2); }
  }
  return opts;
}

const norm = (p) => path.posix.normalize(String(p).replace(/\\/g, '/')).replace(/^\.\//, '');

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
      split: o.split || null,
      definitions: o.definitions || [],
      id: o.id,
      question: o.question,
      expected_files: [...new Set(o.expected_files.map(norm))],
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
    const clean = norm(p).replace(/:\d+(?::\d+)?$/, '');
    if (clean && !seenFile.has(clean)) { seenFile.add(clean); files.push(clean); }
  };
  const pushName = (n) => {
    const clean = n.trim().toLowerCase();
    if (clean && !seenName.has(clean)) { seenName.add(clean); names.push(clean); }
  };
  for (const m of text.matchAll(/^NODE (.+?) \[id=(.*?) src=(.*?) (?:loc|community)=/gm)) {
    pushName(m[1]); pushName(m[2]); pushFile(m[3]);
  }
  for (const m of text.matchAll(/^EDGE .*? @(\S+?):\d+$/gm)) pushFile(m[1]);
  return { files, names };
}

/// Rank (1-based) of the best hit, or Infinity when nothing matched.
function bestRank(expected, ranked) {
  let best = Infinity;
  for (const e of expected) {
    const idx = ranked.findIndex((r) => r === e);
    if (idx !== -1) best = Math.min(best, idx + 1);
  }
  return best;
}

async function main() {
  const opts = parseArgs(process.argv);
  const items = loadGolden(opts.golden);
  const catalog = JSON.parse(readFileSync(path.join(scriptDir, '../reserved-corpora.json'), 'utf8'));
  const reserved = items.some(item => String(item.split || '').startsWith('reserved')) || catalog.golden_files.some(file => path.basename(file) === path.basename(opts.golden));

  // Validate expectations against the real tree — golden paths must exist.
  const treeFiles = walkFiles(opts.root);
  const bad = [];
  for (const item of items) {
    for (const e of item.expected_files) {
      if (!treeFiles.includes(e)) bad.push(`${item.id}: no file matches "${e}"`);
    }
  }
  if (bad.length) {
    console.error(`golden set has ${bad.length} ungrounded expectation(s):`);
    for (const b of bad) console.error('  ' + b);
    process.exit(1);
  }
  console.log(`golden set ok: ${items.length} questions, all expectations grounded`);
  for (const item of items) for (const d of item.definitions) {
    if (!Number.isInteger(d.line) || d.line < 1 || !readFileSync(path.join(opts.root, d.path), 'utf8').split(/\r?\n/)[d.line - 1]?.includes(d.contains)) throw new Error('Ungrounded declaration: ' + item.id);
  }
  if (opts.check) return;
  if (reserved && !opts.allowReserved) throw new Error('Reserved cases require --allow-reserved; the first evaluation consumes their unexercised status.');

  if (!items.length || !opts.k.includes(5) || opts.k.some(k => !Number.isInteger(k) || k < 1) || !(Number(opts.budget) > 0)) throw new Error('nonempty golden set, positive budget and k including 5 required');
  const tok = await loadTokenizer();
  if (!tok) throw new Error('quality measurements require js-tiktoken (o200k_base)');
  const cli = opts.astria;
  const invoke = (args, cwd = opts.root) => cli.endsWith('.js')
    ? spawnSync(process.execPath, [path.resolve(cli), ...args], { cwd, encoding: 'utf8', timeout: 120000, maxBuffer: 32 * 1024 * 1024 })
    : spawnSync(cli, args, { cwd, encoding: 'utf8', timeout: 120000, maxBuffer: 32 * 1024 * 1024 });
  const git = (cwd, ...args) => spawnSync('git', args, { cwd, encoding: 'utf8' }).stdout?.trim() || null;
  const localEntry = path.join(repoRoot, 'packages/astria-cli/dist/index.js');
  const nativeArtifact = path.join(repoRoot, 'packages/astria-cli/dist/astria.node');
  const localBuild = path.resolve(cli) === localEntry;
  if (localBuild && (existsSync(path.join(repoRoot, 'packages/astria-cli/astria.node')) || !existsSync(nativeArtifact))) throw new Error('Local benchmark requires dist/astria.node and no package-root astria.node; rebuild the intended native artifact');
  const versionResult = invoke(['--version']);
  if (versionResult.error || versionResult.status !== 0) throw new Error('CLI/native binding failed to load');
  const version = versionResult.stdout.trim();
  const bounded = (text) => {
    if (tok.count(text) <= Number(opts.budget)) return text;
    let lo = 0, hi = text.length;
    while (lo < hi) { const mid = Math.ceil((lo + hi) / 2); if (tok.count(text.slice(0, mid)) <= Number(opts.budget)) lo = mid; else hi = mid - 1; }
    // Drop an incomplete final record so a clipped path cannot become a hit.
    const prefix = text.slice(0, lo);
    let complete = prefix.slice(0, prefix.lastIndexOf('\n') + 1);
    // Token counts of string prefixes are not strictly monotonic under BPE.
    while (tok.count(complete) > Number(opts.budget)) complete = complete.slice(0, complete.lastIndexOf('\n', complete.length - 2) + 1);
    return complete;
  };
  const baseline = question => lexicalBaseline({ root: opts.root, question, tok, budget: Number(opts.budget), iterative: opts.iterative });
  const returnedDefinitions = text => {
    const records = [];
    if (opts.baseline) {
      for (const m of text.matchAll(/^DEFINITION (.+?):(\d+) (.+)$/gm)) records.push({ path: norm(m[1]), line: Number(m[2]), name: m[3] });
    } else {
      for (const line of text.split('\n')) {
        if (!line.startsWith('NODE ')) continue;
        const m = line.match(/src=(.*?) (?:loc|community)=/);
        if (!m) continue;
        const location = Number(line.match(/ loc=L(\d+)/)?.[1] || m[1].match(/:(\d+)(?::\d+)?$/)?.[1]);
        const file = m[1].replace(/:\d+(?::\d+)?$/, '');
        records.push({ path: norm(path.isAbsolute(file) ? path.relative(opts.root, file) : file), line: location + 1 });
      }
    }
    return records.filter((record, index) => records.findIndex(other => other.path === record.path && other.line === record.line) === index);
  };
  if (reserved) {
    mkdirSync(path.dirname(opts.out), { recursive: true });
    writeFileSync(opts.out + '.reserved-exposure.json', JSON.stringify({ started_at: new Date().toISOString(), golden: opts.golden, golden_sha256: createHash('sha256').update(readFileSync(opts.golden)).digest('hex'), status: 'reservation-consumed-before-first-query' }, null, 2) + '\n');
  }
  const results = [];
  for (const item of items) {
    const t0 = Date.now();
    const r = opts.baseline ? baseline(item.question) : invoke(['query', item.question, '--budget', opts.budget, '--depth', opts.depth]);
    const seconds = (Date.now() - t0) / 1000;
    if (r.error || r.status !== 0 || !r.stdout) {
      results.push({ ...item, search_cost: r.costs || null, definitions: item.definitions.map(d => ({ ...d, rank: null })), recall: Object.fromEntries(opts.k.map(k => [k, 0])), hit_rank: null, tokens: 0, error: (r.stderr || String(r.error || `exit ${r.status}: empty output`)).slice(0, 300), seconds: Number(seconds.toFixed(2)) });
      console.error(`  ${item.id} ERROR ${seconds.toFixed(1)}s`);
      continue;
    }
    const delivered = bounded(r.stdout);
    const { files, names: rankedNames } = parseAnswer(delivered);
    const rankedFiles = (opts.baseline ? [...delivered.matchAll(/^FILE (.+)$/gm)].map(m => m[1]) : files).map(f => norm(path.isAbsolute(f) ? path.relative(opts.root, f) : f));
    const fileRank = bestRank(item.expected_files, rankedFiles);
    const symRank = item.expected_symbols.length ? bestRank(item.expected_symbols, rankedNames) : Infinity;
    const hit = fileRank;
    results.push({
      id: item.id,
      question: item.question,
      seconds: Number(seconds.toFixed(2)),
      search_cost: r.costs || null,
      ranked_files: rankedFiles,
      definitions: item.definitions.map(d => ({ ...d, rank: (returnedDefinitions(delivered).findIndex(n => n.path === norm(d.path) && n.line === d.line) + 1) || null })),
      tokens: tok.count(delivered),
      raw_tokens: tok.count(r.stdout),
      clipped: delivered.length !== r.stdout.length,
      recall: Object.fromEntries(opts.k.map(k => [k, item.expected_files.filter(f => rankedFiles.slice(0,k).includes(f)).length / item.expected_files.length])),
      hit_rank: Number.isFinite(hit) ? hit : null,
      matched_file: Number.isFinite(fileRank) ? rankedFiles[fileRank - 1] : null,
      matched_symbol: Number.isFinite(symRank) ? rankedNames[symRank - 1] : null,
    });
    console.log(`  ${item.id} rank=${Number.isFinite(hit) ? hit : 'miss'} ${seconds.toFixed(1)}s`);
  }

  const ok = results.filter(r => !r.error);
  const n = results.length;
  const summary = { questions:n, answered:ok.length, failed:n-ok.length,
    mrr: results.reduce((s,r) => s + (r.hit_rank ? 1/r.hit_rank : 0),0)/n,
    avg_query_seconds: results.reduce((s,r) => s+r.seconds,0)/n,
    avg_tokens: tok ? results.reduce((s,r) => s+r.tokens,0)/n : null };
  for (const k of opts.k) {
    summary[`hit@${k}`] = results.filter(r => r.hit_rank && r.hit_rank <= k).length/n;
    summary[`recall@${k}`] = results.reduce((s,r) => s+r.recall[k],0)/n;
  }

  const definitions = results.flatMap(row => row.definitions || []);
  summary.baseline_partial_failures = results.filter(row => row.search_cost?.failures.length && !row.error).length;
  summary.definition_count = definitions.length;
  summary.definition_mrr = definitions.length ? definitions.reduce((sum, d) => sum + (d.rank ? 1 / d.rank : 0), 0) / definitions.length : null;
  for (const k of opts.k) summary[`definition_recall@${k}`] = definitions.length ? definitions.filter(d => d.rank && d.rank <= k).length / definitions.length : null;
  const payload = {
    schema_version: 3,
    method: opts.baseline ? (opts.iterative ? 'question-rg-iterative-source-v1' : 'question-rg-single-pass-floor-v2') : 'astria',
    reserved_consumed: reserved,
    baseline_policy: opts.baseline ? { max_rounds: opts.iterative ? 3 : 1, max_reads_per_round: opts.iterative ? 6 : 24, max_read_bytes_per_file: 262144, max_matches_per_round: 2000, max_search_output_bytes: 16777216, search_timeout_ms: 30000, refinement: 'calls and imports in read windows only; expectations unavailable to baseline' } : null,
    tokenizer: tok.name,
    context_policy: 'all methods clipped to the same exact token budget, keeping only complete lines',
    provenance: { cli_version:version, harness_commit:git(repoRoot,'rev-parse','HEAD'), source_build_commit: localBuild ? git(repoRoot,'rev-parse','HEAD') : null, native_artifact_sha256: localBuild ? createHash('sha256').update(readFileSync(nativeArtifact)).digest('hex') : null, cli_entrypoint_sha256: existsSync(cli) ? createHash('sha256').update(readFileSync(cli)).digest('hex') : null, source_dirty: Boolean(git(repoRoot,'status','--porcelain')), corpus_commit:git(opts.root,'rev-parse','HEAD'), corpus_files:treeFiles.length, golden_sha256:createHash('sha256').update(readFileSync(opts.golden)).digest('hex'), node:process.version, platform:process.platform },
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
