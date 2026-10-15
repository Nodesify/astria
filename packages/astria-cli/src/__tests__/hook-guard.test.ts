/**
 * hook-guard contextual-guidance tests — validates the symbol index build
 * and cache, search-pattern extraction, symbol matching/ranking, file
 * matching with native path shapes, formatting bounds, and once-per-key
 * emission. Uses temp .astria fixtures.
 *
 * Run with: npx tsx src/__tests__/hook-guard.test.ts
 */

import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

import {
  SymbolEntry,
  extractSearchPattern,
  matchSymbols,
  symbolsInFile,
  displayPath,
  formatSymbolList,
  symbolIndex,
  contextAlreadyEmitted,
  markContextEmitted,
} from '../commands/hook-guard';

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

function tmpDir(prefix: string): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

const INDEX: SymbolEntry[] = [
  { i: 'a::installGitHooks', l: 'installGitHooks()', f: '/repo/src/install/hooks.ts', n: 202, d: 12 },
  { i: 'a::testInstall', l: 'testInstall()', f: '/repo/src/__tests__/hooks.test.ts', n: 60, d: 1 },
  { i: 'a::installGitHooksRaw', l: 'installGitHooksRaw()', f: '/repo/src/install/hooks.ts', n: 287, d: 3 },
  { i: 'a::hookGuard', l: 'hookGuard()', f: '/repo/src/commands/hook-guard.ts', n: 82, d: 30 },
  { i: 'a::POST_MERGE_SCRIPT', l: 'POST_MERGE_SCRIPT', f: '/repo/src/install/hooks.ts', n: 86, d: 2 },
];

function testExtractSearchPattern() {
  assert(extractSearchPattern('rg installGitHooks -t ts') === 'installGitHooks', 'extract: rg pattern');
  assert(extractSearchPattern('grep -rn post-merge .') === 'post-merge', 'extract: grep skips options');
  assert(extractSearchPattern('git grep ASTRIA_HOOK_VERSION') === 'ASTRIA_HOOK_VERSION', 'extract: git grep');
  assert(extractSearchPattern('git log --oneline') === null, 'extract: non-search git command');
  assert(extractSearchPattern('grep "install hooks" src') === 'install hooks', 'extract: quoted pattern rejoined');
  assert(extractSearchPattern('') === null, 'extract: empty command');
  assert(extractSearchPattern('ls -la') === null, 'extract: non-search command');
}

function testMatchSymbols() {
  const exact = matchSymbols(INDEX, 'installGitHooks()', 5);
  assert(exact[0].i === 'a::installGitHooks', 'match: exact label wins');
  assert(exact.some((e) => e.i === 'a::installGitHooksRaw'), 'match: prefix matches included');

  // Degree breaks ties between the same-scored substring hits.
  const partial = matchSymbols(INDEX, 'hook', 5);
  assert(partial[0].i === 'a::hookGuard', 'match: higher degree ranks first');

  // Regex-style patterns fall back to identifier tokens.
  const regex = matchSymbols(INDEX, 'installGitHooks\\s*\\(', 5);
  assert(regex.some((e) => e.i === 'a::installGitHooks'), 'match: regex pattern matches via token');

  const none = matchSymbols(INDEX, 'zzz_nothing', 5);
  assert(none.length === 0, 'match: no match yields empty');
  assert(matchSymbols(INDEX, 'install', 2).length === 2, 'match: limit respected');
}

function testSymbolsInFile() {
  // The graph stores forward-slash absolute paths; reads arrive in the
  // host's native shape (backslashes on Windows).
  const root = path.sep === '\\' ? 'C:\\repo' : '/repo';
  const storedRoot = root.replace(/\\/g, '/');
  const idx = INDEX.map((e) => ({ ...e, f: e.f.replace('/repo', storedRoot) }));
  const native = path.join(root, 'src', 'install', 'hooks.ts');
  const hits = symbolsInFile(idx, root, native);
  assert(hits.length === 3, 'file match: all symbols in the file found');
  assert(hits[0].d >= hits[1].d && hits[1].d >= hits[2].d, 'file match: hub-first ordering');
  assert(symbolsInFile(idx, root, path.join(root, 'src', 'other.ts')).length === 0, 'file match: other file empty');

  // Defensive branch: graphs storing relative source paths still match.
  const relHits = symbolsInFile(INDEX, '/repo', '/repo/src/install/hooks.ts');
  assert(relHits.length === 3, 'file match: relative stored paths resolve against root');
}

function testDisplayAndFormat() {
  assert(displayPath('/repo', '/repo/src/a.ts') === 'src/a.ts', 'display: root stripped');
  assert(displayPath('/repo', '/elsewhere/src/a.ts') === '/elsewhere/src/a.ts', 'display: foreign root kept');
  const text = formatSymbolList(INDEX.slice(0, 3), '/repo', 60);
  assert(text.includes('installGitHooks() (src/install/hooks.ts:202)'), 'format: label with path:line');
  // The list shrinks until the joined text fits the bound.
  assert(text.length <= 60 + 80, 'format: bounded output');
}

function testSymbolIndexBuildAndCache() {
  const dir = tmpDir('astria-guard-idx-');
  const gdir = path.join(dir, '.astria');
  fs.mkdirSync(gdir, { recursive: true });
  const graph = {
    _meta: {},
    nodes: [
      { id: 'f', label: 'hooks.ts', file_type: 'code', source_file: dir + '/src/hooks.ts', source_line: null },
      { id: 'f::a', label: 'alpha()', file_type: 'code', source_file: dir + '/src/hooks.ts', source_line: 1 },
      { id: 'f::b', label: '{closure#1}()', file_type: 'code', source_file: dir + '/src/hooks.ts', source_line: 5 },
      { id: 'f::c', label: 'chunk text', file_type: 'chunk', source_file: dir + '/doc.md', source_line: 2 },
      { id: 'f::d', label: 'beta()', file_type: 'code', source_file: null, source_line: null },
      { id: 'f::e', label: 'gamma()', file_type: 'test', source_file: dir + '/src/hooks.test.ts', source_line: 9 },
    ],
    edges: [
      { source: 'f::a', target: 'f::e' },
      { source: 'x', target: 'f::a' },
    ],
  };
  fs.writeFileSync(path.join(gdir, 'graph.json'), JSON.stringify(graph));

  const idx = symbolIndex(gdir);
  assert(idx !== null, 'index: built');
  assert(idx !== null && idx.length === 2, 'index: only located code/test symbols with :: ids');
  assert(idx !== null && idx.some((e) => e.l === 'alpha()' && e.d === 2), 'index: degree counted');
  const cache = path.join(gdir, 'cache', 'hook-index.json');
  assert(fs.existsSync(cache), 'index: cache file written');

  // The cached copy serves when it is not older than the graph.
  const again = symbolIndex(gdir);
  assert(again !== null && again.length === 2, 'index: cache round-trips');

  // A missing graph fails open to null.
  const empty = tmpDir('astria-guard-empty-');
  assert(symbolIndex(path.join(empty, '.astria')) === null, 'index: missing graph yields null');
}

function testContextEmissionDedupe() {
  const dir = tmpDir('astria-guard-ctx-');
  const gdir = path.join(dir, '.astria');
  fs.mkdirSync(path.join(gdir, 'cache'), { recursive: true });

  assert(!contextAlreadyEmitted(gdir, 'search:foo', 1800), 'dedupe: unseen key not emitted');
  markContextEmitted(gdir, 'search:foo', 1800);
  assert(contextAlreadyEmitted(gdir, 'search:foo', 1800), 'dedupe: marked key counts as emitted');
  assert(!contextAlreadyEmitted(gdir, 'search:bar', 1800), 'dedupe: other key unaffected');
  assert(!contextAlreadyEmitted(gdir, 'search:foo', 0), 'dedupe: zero TTL always re-emits');
}

function main() {
  testExtractSearchPattern();
  testMatchSymbols();
  testSymbolsInFile();
  testDisplayAndFormat();
  testSymbolIndexBuildAndCache();
  testContextEmissionDedupe();

  console.log(`\n${passed} passed, ${failed} failed (hook-guard)`);
  if (failed > 0) process.exit(1);
}

main();
