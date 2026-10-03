/**
 * Merge-driver tests — the 3-way union merge for .astria/graph.json.
 * Pure logic tests (no git, no filesystem state): both branches' additions
 * survive, deletions are respected, field conflicts resolve 3-way, and the
 * driver exits 0 on mergeable input and 1 on unparseable input.
 *
 * Run with: npx tsx src/__tests__/merge-driver.test.ts
 */

import { mkdtempSync, rmSync, writeFileSync, readFileSync, existsSync } from 'fs';
import { tmpdir } from 'os';
import { join } from 'path';
import { execFileSync } from 'child_process';
import { threeWayUnion } from '../commands/merge-driver';

let passed = 0;
let failed = 0;

function assert(condition: boolean, message: string) {
  if (condition) {
    passed++;
  } else {
    failed++;
    console.error(`FAIL: ${message}`);
  }
}

const byId = (n: any) => String(n.id);
const byEdge = (e: any) => `${e.source}|${e.target}|${e.relation}`;

// ---- 1. Both sides' node additions survive ----
{
  const merged = threeWayUnion(
    [{ id: 'a' }],
    [{ id: 'a' }, { id: 'ours-only' }],
    [{ id: 'a' }, { id: 'theirs-only' }],
    byId,
  );
  const ids = merged.map((n) => n.id).sort();
  assert(
    JSON.stringify(ids) === JSON.stringify(['a', 'ours-only', 'theirs-only']),
    `union keeps additions from both sides, got ${JSON.stringify(ids)}`,
  );
}

// ---- 2. A side's deletion is respected ----
{
  const merged = threeWayUnion(
    [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
    [{ id: 'a' }], // ours deleted b
    [{ id: 'a' }, { id: 'c' }], // theirs deleted c
    byId,
  );
  assert(
    JSON.stringify(merged.map((n) => n.id)) === JSON.stringify(['a']),
    `deletions from either side drop entries, got ${JSON.stringify(merged.map((n) => n.id))}`,
  );
}

// ---- 3. Field conflicts resolve 3-way (unchanged side takes the change) ----
{
  const merged = threeWayUnion(
    [{ id: 'a', label: 'old', docstring: 'same' }],
    [{ id: 'a', label: 'ours-new', docstring: 'same' }],
    [{ id: 'a', label: 'old', docstring: 'same' }],
    byId,
  );
  assert(merged[0].label === 'ours-new', 'only ours changed → ours wins');
}

{
  // Both sides re-labeled the same node differently → ours (git convention).
  const merged = threeWayUnion(
    [{ id: 'a', label: 'old' }],
    [{ id: 'a', label: 'ours-label' }],
    [{ id: 'a', label: 'theirs-label' }],
    byId,
  );
  assert(merged[0].label === 'ours-label', 'both changed → ours wins');
}

{
  // Community renumbering: ours kept base's community, theirs re-clustered.
  const merged = threeWayUnion(
    [{ id: 'a', community: 3 }],
    [{ id: 'a', community: 3 }],
    [{ id: 'a', community: 7 }],
    byId,
  );
  assert(merged[0].community === 7, 'unchanged side takes the changed side');
}

// ---- 4. Edge keying: same pair, different relation = distinct edges ----
{
  const merged = threeWayUnion(
    [],
    [{ source: 'a', target: 'b', relation: 'calls' }],
    [{ source: 'a', target: 'b', relation: 'imports' }],
    byEdge,
  );
  assert(merged.length === 2, 'distinct relations are distinct edges');
}

// ---- 5. The git entry point: exit 0 + merged file written in place ----
{
  const dir = mkdtempSync(join(tmpdir(), 'astria-merge-driver-'));
  try {
    const write = (name: string, graph: any) => {
      const p = join(dir, name);
      writeFileSync(p, JSON.stringify(graph));
      return p;
    };
    const base = write('base.json', { nodes: [{ id: 'a' }], edges: [], hyperedges: [], communities: [] });
    const ours = write('ours.json', { nodes: [{ id: 'a' }, { id: 'new-ours' }], edges: [], hyperedges: [], communities: [] });
    const theirs = write('theirs.json', { nodes: [{ id: 'a' }, { id: 'new-theirs' }], edges: [], hyperedges: [], communities: [] });

    const cli = join(__dirname, '..', '..', 'dist', 'index.js');
    let code = 0;
    let ran = false;
    if (existsSync(cli)) {
      try {
        execFileSync('node', [cli, 'merge-driver', 'run', base, ours, theirs], { stdio: 'pipe' });
        ran = true;
      } catch (e: any) {
        ran = true;
        code = e.status ?? 1;
      }
    }
    if (ran) {
      assert(code === 0, `merge-driver run should exit 0, got ${code}`);
      const mergedGraph = JSON.parse(readFileSync(ours, 'utf-8'));
      const ids = mergedGraph.nodes.map((n: any) => n.id).sort();
      assert(
        JSON.stringify(ids) === JSON.stringify(['a', 'new-ours', 'new-theirs']),
        `run writes the union into ours' path, got ${JSON.stringify(ids)}`,
      );
    } else {
      console.log('(dist not built - skipping CLI invocation check)');
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

// ---- 6. Unparseable input exits 1 (git falls back to a conflict) ----
{
  const dir = mkdtempSync(join(tmpdir(), 'astria-merge-driver-'));
  try {
    const base = join(dir, 'base.json');
    const ours = join(dir, 'ours.json');
    const theirs = join(dir, 'theirs.json');
    writeFileSync(base, '{}');
    writeFileSync(ours, '{broken json');
    writeFileSync(theirs, '{}');
    const cli = join(__dirname, '..', '..', 'dist', 'index.js');
    if (existsSync(cli)) {
      let code = 0;
      try {
        execFileSync('node', [cli, 'merge-driver', 'run', base, ours, theirs], { stdio: 'pipe' });
      } catch (e: any) {
        code = e.status ?? 1;
      }
      assert(code === 1, `unparseable input must exit 1, got ${code}`);
    } else {
      console.log('(dist not built - skipping CLI invocation check)');
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

// Summary
console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
  process.exit(1);
}
