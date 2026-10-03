// Git merge driver for the knowledge graph artifact. Parallel branches both
// rebuild the graph and both commit `.astria/graph.json` — plain git sees
// JSON line noise and conflicts. This driver performs a 3-way union merge
// over the graph structure so both branches' additions survive:
//
//   nodes/edges/hyperedges: union by key with classic 3-way field
//     resolution (unchanged side takes the changed side; both changed keeps
//     ours); a side's deletion of an entry is respected.
//   communities: derived data — take whichever side changed from base;
//     the next `astria update` re-clusters anyway.
//
// Git wiring (done by `astria merge-driver install`):
//   .gitattributes:  .astria/graph.json merge=astria
//   .git/config:     merge.astria.driver = astria merge-driver run %O %A %B %P
//
// Exit 0 = merged cleanly (the merged graph is written over %A); exit 1 =
// unmergeable input (git falls back to a conflict).

import { existsSync, readFileSync, writeFileSync } from 'fs';
import { execFileSync } from 'child_process';
import { join } from 'path';

type Json = Record<string, any>;

const GRAPH_JSON_PATH = '.astria/graph.json';
const REPORT_PATH = '.astria/graph_report.md';

function loadJson(path: string, label: string): Json {
  try {
    return JSON.parse(readFileSync(path, 'utf-8'));
  } catch (e: any) {
    throw new Error(`cannot read ${label} (${path}): ${e.message || e}`);
  }
}

/// Classic 3-way field resolution for two versions of one object.
/// Unchanged-on-both-sides, changed-on-one-side, and agree-on-both cases
/// collapse; genuine divergence keeps ours (git's own convention).
function mergeFields(base: Json | undefined, ours: Json, theirs: Json): Json {
  const out: Json = {};
  const keys = new Set([...Object.keys(ours), ...Object.keys(theirs)]);
  for (const key of keys) {
    const o = ours[key];
    const t = theirs[key];
    const b = base ? base[key] : undefined;
    if (JSON.stringify(o) === JSON.stringify(t)) {
      out[key] = o;
    } else if (JSON.stringify(o) === JSON.stringify(b)) {
      out[key] = t; // only theirs changed
    } else if (JSON.stringify(t) === JSON.stringify(b)) {
      out[key] = o; // only ours changed
    } else {
      out[key] = o; // both changed differently — ours wins
    }
  }
  return out;
}

/// 3-way union over arrays of objects keyed by `key(entry)`. An entry that
/// exists only on one side and not in base is that side's addition; one in
/// base but missing from a side is that side's deletion.
export function threeWayUnion(
  baseEntries: Json[],
  oursEntries: Json[],
  theirsEntries: Json[],
  keyOf: (entry: Json) => string,
): Json[] {
  const index = (entries: Json[]) => new Map(entries.map((e) => [keyOf(e), e]));
  const base = index(baseEntries);
  const ours = index(oursEntries);
  const theirs = index(theirsEntries);
  const keys = new Set([...base.keys(), ...ours.keys(), ...theirs.keys()]);
  const merged: Json[] = [];
  for (const key of [...keys].sort()) {
    const b = base.get(key);
    const o = ours.get(key);
    const t = theirs.get(key);
    if (o && t) {
      merged.push(mergeFields(b, o, t));
    } else if (o) {
      if (!b) merged.push(o); // ours added it
      // else theirs deleted it — drop
    } else if (t) {
      if (!b) merged.push(t); // theirs added it
      // else ours deleted it — drop
    }
    // both deleted, or vanished both sides — drop
  }
  return merged;
}

/// Communities are derived (re-clustered on every build): whichever side
/// moved away from base wins wholesale; identical sides stay identical.
function mergeCommunities(base: Json | undefined, ours: Json[], theirs: Json[]): Json[] {
  if (base && JSON.stringify(ours) === JSON.stringify(base.communities)) return theirs;
  return ours;
}

function mergeGraphs(base: Json, ours: Json, theirs: Json): Json {
  const arr = (graph: Json, name: string): Json[] =>
    Array.isArray(graph[name]) ? graph[name] : [];
  return {
    nodes: threeWayUnion(arr(base, 'nodes'), arr(ours, 'nodes'), arr(theirs, 'nodes'), (n) => String(n.id)),
    edges: threeWayUnion(
      arr(base, 'edges'),
      arr(ours, 'edges'),
      arr(theirs, 'edges'),
      (e) => `${e.source}|${e.target}|${e.relation}`,
    ),
    hyperedges: threeWayUnion(
      arr(base, 'hyperedges'),
      arr(ours, 'hyperedges'),
      arr(theirs, 'hyperedges'),
      (h) => String(h.id),
    ),
    communities: mergeCommunities(base, arr(ours, 'communities'), arr(theirs, 'communities')),
  };
}

/// The `git merge` driver entry: base, ours (written in place), theirs,
/// then git's path argument.
export function mergeDriverRun(basePath: string, oursPath: string, theirsPath: string): number {
  try {
    const base = existsSync(basePath) ? loadJson(basePath, 'base graph') : { nodes: [], edges: [], hyperedges: [], communities: [] };
    const ours = loadJson(oursPath, 'ours');
    const theirs = loadJson(theirsPath, 'theirs');
    const merged = mergeGraphs(base, ours, theirs);
    writeFileSync(oursPath, JSON.stringify(merged, null, 2) + '\n');
    const n = merged.nodes.length;
    const e = merged.edges.length;
    console.log(`astria merge-driver: union-merged graph.json (${n} nodes, ${e} edges)`);
    return 0;
  } catch (e: any) {
    console.error(`astria merge-driver: ${e.message || e}`);
    console.error('falling back to a git conflict on .astria/graph.json');
    return 1;
  }
}

function gitConfigGet(key: string): string | null {
  try {
    return execFileSync('git', ['config', '--get', key], { encoding: 'utf-8' }).trim() || null;
  } catch {
    return null;
  }
}

/// Idempotent install: .gitattributes entry + git config driver definition.
export function mergeDriverInstall(projectDir: string): string[] {
  const messages: string[] = [];
  const attributesPath = join(projectDir, '.gitattributes');
  const graphEntry = `${GRAPH_JSON_PATH} merge=astria`;
  const reportEntry = `${REPORT_PATH} merge=union`;
  let attributes = existsSync(attributesPath) ? readFileSync(attributesPath, 'utf-8') : '';
  let changed = false;
  for (const entry of [graphEntry, reportEntry]) {
    if (!attributes.split('\n').some((line) => line.trim() === entry)) {
      if (attributes && !attributes.endsWith('\n')) attributes += '\n';
      attributes += entry + '\n';
      changed = true;
      messages.push(`.gitattributes: ${entry}`);
    }
  }
  if (changed) writeFileSync(attributesPath, attributes);

  execFileSync('git', ['config', 'merge.astria.name', 'astria knowledge graph union-merge'], { cwd: projectDir });
  execFileSync(
    'git',
    ['config', 'merge.astria.driver', 'astria merge-driver run %O %A %B %P'],
    { cwd: projectDir },
  );
  messages.push('git config: merge.astria.driver = astria merge-driver run %O %A %B %P');
  messages.push('committed .astria/graph.json files on parallel branches now union-merge automatically');
  return messages;
}

export function mergeDriverUninstall(projectDir: string): string[] {
  const messages: string[] = [];
  const attributesPath = join(projectDir, '.gitattributes');
  if (existsSync(attributesPath)) {
    const kept = readFileSync(attributesPath, 'utf-8')
      .split('\n')
      .filter((line) => {
        const t = line.trim();
        return t !== `${GRAPH_JSON_PATH} merge=astria` && t !== `${REPORT_PATH} merge=union` && t !== '';
      });
    writeFileSync(attributesPath, kept.length ? kept.join('\n') + '\n' : '');
    messages.push('.gitattributes: astria merge entries removed');
  }
  for (const key of ['merge.astria.name', 'merge.astria.driver']) {
    try {
      execFileSync('git', ['config', '--unset', key], { cwd: projectDir, stdio: 'pipe' });
      messages.push(`git config: ${key} removed`);
    } catch {
      // never set — fine
    }
  }
  return messages;
}
