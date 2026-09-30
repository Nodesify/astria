#!/usr/bin/env node
// Judge-layer A/B benchmark: does `--judge jev` change what the graph
// retrieves, and what it costs?
//
// Builds (or reuses) one graph per mode over the same corpus —
//   plain    structural extraction only, no LLM
//   llm      semantic extraction via an OpenAI-compatible backend
//   llm-jev  the same backend with the Jev judge layered on top
// — then scores every golden set against every mode at every budget and
// detail tier with the same NODE-line file-rank methodology as
// run-quality.mjs. Graph shape comes from `astria stats --json`; build
// output tails are recorded so gate/verify summaries survive into the
// results JSON.
//
// The judge gates and re-judges with a live decision model, so a judged
// graph is one sample from a distribution (measured gate variance:
// 31 vs 17 files gated on identical replay in the 2026-09-28 modes run).
// Treat single-run deltas as directional; rerun before claiming a trend.
//
// Usage:
//   node scripts/bench/quality/judge-ab.mjs --corpus <dir> [options]
//
//   --corpus <dir>            source corpus; copied per mode into --work
//                             (required unless every mode has --mode-dir)
//   --mode-dir <mode>=<path>  reuse a prebuilt corpus dir for that mode
//                             (its .astria graph is scored as-is; repeatable)
//   --modes plain,llm,llm-jev modes to build and score (default all three)
//   --golden <jsonl>          golden set file; repeatable (default: the
//                             Click family — frozen, heldout-v1/v2,
//                             doc-intent-v1; click.reserved-v1 stays reserved)
//   --budgets 1000,4000       query token budgets to sweep
//   --details default,high    detail tiers to sweep ('' maps to default)
//   --depth 2                 traversal depth
//   --work <dir>              work dir for per-mode copies
//                             (default bench-work/judge-ab/<timestamp>)
//   --out <path>              results JSON (default <work>/results.json)
//   --report <path>           markdown report (default <work>/report.md)
//   --astria <path>           CLI entry (default packages/astria-cli/dist/index.js)
//   --api-key-file <path>     engine key file for llm/llm-jev builds
//                             (default bench-work/llm-exp/or_key.txt, OpenRouter)
//   --base-url <url>          engine base URL (default https://openrouter.ai/api/v1)
//   --model <name>            engine model (default openai/gpt-4o-mini)
//   --judge-key-file <path>   Typesafe key file for llm-jev builds; env
//                             TYPESAFE_API_KEY / ASTRIA_LLM_JUDGE_API_KEY
//                             also work
//
// plain needs no key. llm needs the engine key. llm-jev needs both the
// engine key and a Typesafe key; a missing key is a hard error before any
// build spend happens.

import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const scriptDir = import.meta.dirname;
const repoRoot = path.resolve(scriptDir, '..', '..', '..');

const DEFAULT_GOLDENS = [
  ['scripts/bench/external/click.jsonl', 'frozen'],
  ['scripts/bench/paired/click.heldout-v1.jsonl', 'heldout-v1'],
  ['scripts/bench/paired/click.heldout-v2.jsonl', 'heldout-v2'],
  ['scripts/bench/paired/click.doc-intent-v1.jsonl', 'doc-intent'],
];

function parseArgs(argv) {
  const opts = {
    corpus: null,
    modeDirs: new Map(),
    modes: 'plain,llm,llm-jev',
    goldens: [],
    budgets: '1000,4000',
    details: 'default,high',
    depth: '2',
    work: null,
    out: null,
    report: null,
    astria: path.join(repoRoot, 'packages', 'astria-cli', 'dist', 'index.js'),
    apiKeyFile: path.join(repoRoot, 'bench-work', 'llm-exp', 'or_key.txt'),
    baseUrl: 'https://openrouter.ai/api/v1',
    model: 'openai/gpt-4o-mini',
    judgeKeyFile: null,
  };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--corpus') opts.corpus = path.resolve(argv[++i]);
    else if (a === '--mode-dir') {
      const [mode, dir] = argv[++i].split('=');
      opts.modeDirs.set(mode, path.resolve(dir));
    } else if (a === '--modes') opts.modes = argv[++i];
    else if (a === '--golden') opts.goldens.push(path.resolve(argv[++i]));
    else if (a === '--budgets') opts.budgets = argv[++i];
    else if (a === '--details') opts.details = argv[++i];
    else if (a === '--depth') opts.depth = argv[++i];
    else if (a === '--work') opts.work = path.resolve(argv[++i]);
    else if (a === '--out') opts.out = path.resolve(argv[++i]);
    else if (a === '--report') opts.report = path.resolve(argv[++i]);
    else if (a === '--astria') opts.astria = path.resolve(argv[++i]);
    else if (a === '--api-key-file') opts.apiKeyFile = path.resolve(argv[++i]);
    else if (a === '--base-url') opts.baseUrl = argv[++i];
    else if (a === '--model') opts.model = argv[++i];
    else if (a === '--judge-key-file') opts.judgeKeyFile = path.resolve(argv[++i]);
    else { console.error(`unknown arg: ${a}`); process.exit(2); }
  }
  opts.modes = opts.modes.split(',').map(m => m.trim()).filter(Boolean);
  opts.budgets = opts.budgets.split(',').map(Number);
  opts.details = opts.details.split(',').map(d => (d === 'default' ? '' : d));
  if (!opts.goldens.length) opts.goldens = DEFAULT_GOLDENS.map(([rel]) => path.join(repoRoot, rel));
  if (!opts.work) opts.work = path.join(repoRoot, 'bench-work', 'judge-ab', new Date().toISOString().replace(/[:.]/g, '-'));
  if (!opts.out) opts.out = path.join(opts.work, 'results.json');
  if (!opts.report) opts.report = path.join(opts.work, 'report.md');
  return opts;
}

const norm = (p) => path.posix.normalize(String(p).replace(/\\/g, '/')).replace(/^\.\//, '');

function loadGolden(file, set) {
  return readFileSync(file, 'utf8').trim().split('\n').map((line, i) => {
    let o;
    try { o = JSON.parse(line); } catch (e) { throw new Error(`${file}:${i + 1} invalid JSON: ${e.message}`); }
    if (typeof o.question !== 'string' || !Array.isArray(o.expected_files) || !o.expected_files.length)
      throw new Error(`${file}:${i + 1} needs string "question" and non-empty "expected_files"`);
    return {
      id: o.id ?? `${set}-${i}`,
      question: o.question,
      expected_files: [...new Set(o.expected_files.map(norm))],
      set,
    };
  });
}

/// Parse ranked source files out of query NODE output. NODE lines look like
/// `NODE <label> [id=<id> src=<path[:line]> community=<n>]`.
function rankedFiles(text, cwd) {
  const out = [];
  for (const line of text.split('\n')) {
    if (!line.startsWith('NODE ')) continue;
    const m = line.match(/\bsrc=(.*?) (?:loc|community)=/);
    if (!m) continue;
    let f = m[1].replace(/:\d+(?::\d+)?$/, '').replaceAll('\\', '/');
    if (path.isAbsolute(f)) f = path.relative(cwd, f).replaceAll('\\', '/');
    f = norm(f);
    if (f && !out.includes(f)) out.push(f);
  }
  return out;
}

function bestRank(expected, ranked) {
  let best = Infinity;
  for (const e of expected) {
    const idx = ranked.indexOf(e);
    if (idx !== -1) best = Math.min(best, idx + 1);
  }
  return best;
}

const run = (cmd, args, opts = {}) =>
  spawnSync(cmd, args, { encoding: 'utf8', timeout: 600_000, maxBuffer: 64 * 1024 * 1024, ...opts });

function engineEnv(opts) {
  const env = { ...process.env, NODE_PATH: path.join(repoRoot, 'node_modules') };
  if (!existsSync(opts.apiKeyFile))
    throw new Error(`engine key file not found: ${opts.apiKeyFile} (llm/llm-jev builds bill the engine backend)`);
  env.ASTRIA_LLM_BACKEND = 'openai';
  env.ASTRIA_LLM_MODEL = opts.model;
  env.ASTRIA_LLM_BASE_URL = opts.baseUrl;
  env.ASTRIA_LLM_API_KEY = readFileSync(opts.apiKeyFile, 'utf8').trim();
  return env;
}

function judgeKey(opts) {
  if (opts.judgeKeyFile && existsSync(opts.judgeKeyFile))
    return readFileSync(opts.judgeKeyFile, 'utf8').trim();
  for (const name of ['ASTRIA_LLM_JUDGE_API_KEY', 'TYPESAFE_API_KEY'])
    if (process.env[name]) return process.env[name];
  return null;
}

function copyCorpus(src, dst) {
  mkdirSync(path.dirname(dst), { recursive: true });
  cpSync(src, dst, {
    recursive: true,
    filter: (p) => {
      const base = path.basename(p);
      return base !== '.git' && base !== '.astria' && base !== '.graphify';
    },
  });
}

function buildMode(mode, dir, opts) {
  let env = { ...process.env, NODE_PATH: path.join(repoRoot, 'node_modules') };
  if (mode === 'llm' || mode === 'llm-jev') {
    env = engineEnv(opts);
    if (mode === 'llm-jev') {
      const key = judgeKey(opts);
      if (!key)
        throw new Error(
          'llm-jev needs a Typesafe key: set TYPESAFE_API_KEY / ASTRIA_LLM_JUDGE_API_KEY or pass --judge-key-file'
        );
      env.ASTRIA_LLM_JUDGE = 'jev';
      env.ASTRIA_LLM_JUDGE_API_KEY = key;
    }
  }
  const t0 = Date.now();
  const r = run(process.execPath, [opts.astria, 'run', dir], { cwd: dir, env });
  const seconds = ((Date.now() - t0) / 1000).toFixed(1);
  if (r.status !== 0) throw new Error(`${mode} build failed (exit ${r.status}): ${(r.stderr || '').slice(0, 400)}`);
  return { seconds, build_log_tail: (r.stdout || '').trim().split('\n').slice(-40).join('\n') };
}

function graphStats(dir, opts) {
  const r = run(process.execPath, [opts.astria, 'stats', '--json', '--graph', dir], { cwd: dir });
  if (r.status !== 0 || !r.stdout) return { error: (r.stderr || 'empty stats output').slice(0, 200) };
  try { return JSON.parse(r.stdout); } catch { return { raw: r.stdout.slice(0, 300) }; }
}

function main() {
  const opts = parseArgs(process.argv);
  const goldens = opts.goldens.flatMap(file => {
    const known = DEFAULT_GOLDENS.find(([rel]) => path.join(repoRoot, rel) === file);
    return loadGolden(file, known ? known[1] : path.basename(file, '.jsonl'));
  });

  // Pre-flight every mode's requirements before spending anything.
  for (const mode of opts.modes) {
    if (opts.modeDirs.has(mode)) continue;
    if (!opts.corpus) throw new Error(`mode ${mode} has no --mode-dir and no --corpus to copy from`);
    if (!existsSync(opts.corpus)) throw new Error(`--corpus not found: ${opts.corpus}`);
    if (mode === 'llm-jev' && !judgeKey(opts))
      throw new Error('llm-jev needs a Typesafe key: set TYPESAFE_API_KEY / ASTRIA_LLM_JUDGE_API_KEY or pass --judge-key-file');
    if ((mode === 'llm' || mode === 'llm-jev') && !existsSync(opts.apiKeyFile))
      throw new Error(`engine key file not found: ${opts.apiKeyFile}`);
  }

  const version = run(process.execPath, [opts.astria, '--version']);
  if (version.status !== 0) throw new Error('CLI failed to load');

  mkdirSync(opts.work, { recursive: true });
  const results = { modes: {}, sets: {} };
  for (const mode of opts.modes) {
    const dir = opts.modeDirs.get(mode) ?? path.join(opts.work, mode);
    if (!opts.modeDirs.has(mode)) {
      copyCorpus(opts.corpus, dir);
      console.log(`[${mode}] building graph in ${dir} ...`);
      const build = buildMode(mode, dir, opts);
      console.log(`[${mode}] built in ${build.seconds}s`);
      results.modes[mode] = { dir, prebuilt: false, build_seconds: Number(build.seconds), build_log_tail: build.build_log_tail, stats: graphStats(dir, opts) };
    } else {
      console.log(`[${mode}] using prebuilt ${dir}`);
      results.modes[mode] = { dir, prebuilt: true, stats: graphStats(dir, opts) };
    }
  }

  const cells = [];
  for (const mode of opts.modes) {
    const dir = results.modes[mode].dir;
    for (const detail of opts.details) {
      for (const item of goldens) {
        const perBudget = {};
        for (const budget of opts.budgets) {
          const args = [opts.astria, 'query', item.question, '--budget', String(budget), '--depth', opts.depth];
          if (detail) args.push('--detail', detail);
          const q = run(process.execPath, args, { cwd: dir, env: { ...process.env, NODE_PATH: path.join(repoRoot, 'node_modules') } });
          if (q.status !== 0 || !q.stdout) {
            perBudget[budget] = { error: (q.stderr || `exit ${q.status}`).slice(0, 200) };
            continue;
          }
          const ranked = rankedFiles(q.stdout, dir);
          const rank = bestRank(item.expected_files, ranked);
          perBudget[budget] = {
            rank: Number.isFinite(rank) ? rank : null,
            recall: item.expected_files.filter(f => ranked.slice(0, 5).includes(f)).length / item.expected_files.length,
          };
        }
        cells.push({ mode, detail: detail || 'default', set: item.set, id: item.id, perBudget });
        process.stdout.write(`  ${mode}/${detail || 'default'}/${item.set} ${item.id} done\n`);
      }
    }
  }

  // Aggregate: per (set, budget, detail, mode) → hit@1, recall@5, MRR.
  const summary = [];
  for (const set of [...new Set(goldens.map(g => g.set))]) {
    for (const budget of opts.budgets) {
      for (const detail of opts.details.map(d => d || 'default')) {
        for (const mode of opts.modes) {
          const rows = cells.filter(c => c.set === set && c.mode === mode && c.detail === detail);
          const ok = rows.filter(r => r.perBudget[budget] && !r.perBudget[budget].error);
          const n = rows.length || 1;
          summary.push({
            set, budget, detail, mode, n: rows.length, errors: rows.length - ok.length,
            hit1: ok.filter(r => r.perBudget[budget].rank !== null && r.perBudget[budget].rank <= 1).length / n,
            recall5: ok.reduce((s, r) => s + r.perBudget[budget].recall, 0) / n,
            mrr: ok.reduce((s, r) => s + (r.perBudget[budget].rank ? 1 / r.perBudget[budget].rank : 0), 0) / n,
          });
        }
      }
    }
  }

  const payload = {
    schema_version: 1,
    benchmark: 'judge-layer A/B (modes: plain / llm / llm-jev)',
    generated_at: new Date().toISOString(),
    provenance: {
      cli_version: version.stdout.trim(),
      astria: opts.astria,
      engine: { backend: 'openai', base_url: opts.baseUrl, model: opts.model, key_file: opts.apiKeyFile },
      judge: { layer: 'jev (TypeSafe System One)', key_source: judgeKey(opts) ? 'provided' : 'absent' },
      corpus: opts.corpus,
      budgets: opts.budgets, details: opts.details.map(d => d || 'default'), depth: Number(opts.depth),
      goldens: opts.goldens,
      note: 'The Jev gate is a live decision model — judged graphs are one sample from a distribution; rerun before claiming a trend.',
    },
    modes: results.modes,
    summary,
    cells,
  };
  writeFileSync(opts.out, JSON.stringify(payload, null, 2) + '\n');

  const fmt = (x) => (x * 100).toFixed(1) + '%';
  const lines = [
    '# Judge-layer A/B',
    '',
    `Generated ${payload.generated_at}; CLI ${payload.provenance.cli_version}; corpus \`${payload.provenance.corpus ?? 'prebuilt dirs'}\`.`,
    'Columns: hit@1 / recall@5 / MRR per golden set at each budget and detail tier.',
    '',
  ];
  for (const detail of opts.details.map(d => d || 'default')) {
    lines.push(`## detail ${detail}`, '', '| set@budget | ' + opts.modes.map(m => `${m}`).join(' | ') + ' |', '|' + '---|'.repeat(opts.modes.length + 1));
    for (const set of [...new Set(goldens.map(g => g.set))]) {
      for (const budget of opts.budgets) {
        const row = summary.filter(s => s.set === set && s.budget === budget && s.detail === detail);
        lines.push(`| ${set}@${budget} | ` + opts.modes.map(m => {
          const c = row.find(r => r.mode === m);
          return c ? `${fmt(c.hit1)} / ${fmt(c.recall5)} / ${c.mrr.toFixed(3)} (n=${c.n})` : '—';
        }).join(' | ') + ' |');
      }
    }
    lines.push('');
  }
  lines.push('## Graph shape', '', '| mode | nodes | edges | communities | prebuilt |', '|---|---|---|---|---|');
  for (const mode of opts.modes) {
    const s = results.modes[mode].stats ?? {};
    lines.push(`| ${mode} | ${s.nodeCount ?? '?'} | ${s.edgeCount ?? '?'} | ${s.communityCount ?? '?'} | ${results.modes[mode].prebuilt ? 'yes' : 'no'} |`);
  }
  writeFileSync(opts.report, lines.join('\n') + '\n');
  console.log(`\nresults: ${opts.out}\nreport: ${opts.report}`);
}

main();
