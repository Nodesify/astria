// A/B benchmark driver: node driver.mjs <astria.node> <corpus> <label> <round>
// Prints one JSON line with millisecond timings per phase plus graph-size meta.
import { createRequire } from 'node:module';
import { rmSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// No LLM backends, no pins: identical, deterministic runs.
for (const k of Object.keys(process.env)) if (k.startsWith('ASTRIA_')) delete process.env[k];

const [, , modulePath, corpus, label, round] = process.argv;
const require = createRequire(import.meta.url);
const mod = require(modulePath);

const t = {};
const timed = async (name, fn) => {
  const t0 = process.hrtime.bigint();
  const out = await fn();
  t[name] = Number(process.hrtime.bigint() - t0) / 1e6;
  return out;
};

// 1. Cold pipeline: fresh .astria, full extraction + build + cluster + report.
rmSync(join(corpus, '.astria'), { recursive: true, force: true });
const run = await timed('cold_pipeline', () => mod.runPipeline(corpus));
t.nodes_added = run.nodesAdded;
t.edges_added = run.edgesAdded;
t.communities = run.communities;

// 2. No-op update (nothing changed since the cold run).
const upd = await timed('update_noop', () => mod.updatePipeline(corpus));
t.update_files_processed = upd.filesProcessed;

// 3. Stats (one graph load).
await timed('stats', () => mod.graphStats(corpus));

// 4. Five queries. The 1st pays graph load; the rest show in-process
// snapshot-cache behavior (new) vs full reload (old).
const questions = [
  'how does authentication work',
  'what is the main entry point',
  'how are errors handled',
  'data layer api',
  'core abstractions',
];
for (let i = 0; i < questions.length; i++) {
  await timed(`query_${i + 1}`, () =>
    mod.queryGraph(corpus, questions[i], 'bfs', 2, 4000)
  );
}

// 5. Repo map + explain + exports.
await timed('repo_map', () => mod.repoMap(corpus, 2000));
if (typeof mod.godNodes === 'function') {
  const gods = await timed('god_nodes', () => mod.godNodes(corpus));
  if (typeof mod.explainNode === 'function' && gods.length > 0 && gods[0].id) {
    await timed('explain_node', () => mod.explainNode(corpus, gods[0].id));
  }
}
const outdir = mkdtempSync(join(tmpdir(), 'astria-bench-'));
await timed('export_json', () => mod.exportJsonCmd(corpus, join(outdir, 'g.json')));
await timed('export_html', () => mod.exportHtmlCmd(corpus, join(outdir, 'g.html'), 'large'));
rmSync(outdir, { recursive: true, force: true });

console.log(JSON.stringify({ label, round: Number(round), ms: t }));
