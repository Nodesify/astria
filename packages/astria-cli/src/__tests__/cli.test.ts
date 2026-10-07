/**
 * CLI test — validates the REAL Commander program from src/index.ts:
 * every command registered, expected options present, and the version
 * in sync with package.json. Does not execute any command actions.
 *
 * Run with: npx tsx src/__tests__/cli.test.ts
 */

import { execFileSync } from 'child_process';
import { existsSync } from 'fs';
import { join } from 'path';
import { Command } from 'commander';
import { program } from '../index';
import { DEFAULT_QUERY_BUDGET } from '../defaults';

// eslint-disable-next-line @typescript-eslint/no-var-requires
const pkg = require('../../package.json');

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

// ---- The real program ----
// Importing index.ts registers every command AND loads the native binding,
// so this test also fails fast when the native module is broken.

// Test 1: every command the CLI ships is registered — and nothing else.
// Set-equality on purpose: an inclusion-only list drifts silently (it had
// missed 11 shipped commands, the same drift that let the npm README keep
// advertising the removed `astria migrate`). When the command surface
// changes, update this list; a mismatch fails the suite in both directions.
const expectedCommands: Set<string> = new Set([
  'add', 'affected', 'callflow', 'cluster-only', 'communities', 'diagnose',
  'diff', 'digest', 'doctor', 'explain', 'export', 'global', 'god-nodes', 'health',
  'history', 'hook', 'hook-guard', 'install', 'map', 'mcp', 'merge',
  'merge-driver', 'merge-gate', 'neighbors', 'path', 'prs', 'query',
  'reflect', 'risk', 'run', 'save-result', 'stats', 'status', 'tree',
  'uninstall', 'update', 'watch', 'wiki',
]);
const actualCommands = new Set(program.commands.map((c: Command) => c.name()));
for (const cmd of expectedCommands) {
  assert(actualCommands.has(cmd), `Command "${cmd}" should be registered`);
}
for (const cmd of actualCommands) {
  assert(
    expectedCommands.has(cmd),
    `Command "${cmd}" is registered but missing from the expected set in cli.test.ts`,
  );
}

// Test 2: version stays in sync with package.json (the stub test used to
// hard-code 0.1.0 while the package moved on to 0.5.0)
assert(
  program.version() === pkg.version,
  `Program version (${program.version()}) should match package.json (${pkg.version})`
);

// Test 3: query carries its full option surface (--directed/--detail/--cursor
// were added in 0.5.0; a mirror of the program cannot see them)
function optsOf(name: string): string[] {
  const cmd = program.commands.find((c: Command) => c.name() === name);
  assert(cmd !== undefined, `Command "${name}" should exist for option check`);
  return cmd ? cmd.options.map((o: any) => o.long) : [];
}

const queryOpts = optsOf('query');
for (const opt of ['--dfs', '--depth', '--budget', '--directed', '--detail', '--cursor', '--graph']) {
  assert(queryOpts.includes(opt), `query should have ${opt}`);
}

const runOpts = optsOf('run');
for (const opt of ['--no-dedup', '--backend', '--model', '--wiki', '--embed']) {
  assert(runOpts.includes(opt), `run should have ${opt}`);
}

const updateOpts = optsOf('update');
for (const opt of ['--no-dedup', '--backend', '--model', '--embed']) {
  assert(updateOpts.includes(opt), `update should have ${opt}`);
}

const pathOpts = optsOf('path');
for (const opt of ['--directed', '--detail', '--graph']) {
  assert(pathOpts.includes(opt), `path should have ${opt}`);
}

const affectedOpts = optsOf('affected');
for (const opt of ['--depth', '--relation', '--graph']) {
  assert(affectedOpts.includes(opt), `affected should have ${opt}`);
}

const exportOpts = optsOf('export');
for (const opt of ['--format', '--mode', '--out', '--graph']) {
  assert(exportOpts.includes(opt), `export should have ${opt}`);
}
const formatOpt = program.commands
  .find((c: Command) => c.name() === 'export')
  ?.options.find((o: any) => o.long === '--format');
assert(!!formatOpt && (formatOpt.defaultValue ?? 'json') === 'json', 'export --format should default to "json"');
const modeOpt = program.commands
  .find((c: Command) => c.name() === 'export')
  ?.options.find((o: any) => o.long === '--mode');
assert(!!modeOpt && (modeOpt.defaultValue ?? 'standard') === 'standard', 'export --mode should default to "standard"');

// Test 3b: napi platform binaries are shipped via optionalDependencies — a
// stale pin makes npm install a previous version's .node binary (0.6.0
// shipped 0.5.0's binary because the pins were not bumped)
const optDeps: Record<string, string> = pkg.optionalDependencies ?? {};
// One pin per published release target. Bump deliberately when a target
// is added or retired — this assert exists to force that update to be
// conscious (and to catch a stale pin, which ships an old .node binary).
// Release gates require all seven native packages before publishing the CLI.
assert(Object.keys(optDeps).length === 7, 'all 7 supported napi platform packages should be pinned');
for (const [name, pinned] of Object.entries(optDeps)) {
  assert(pinned === pkg.version, `${name} pinned at ${pinned} should match package version ${pkg.version}`);
}

for (const opt of ['--author', '--contributor', '--graph']) {
  assert(optsOf('add').includes(opt), `add should have ${opt}`);
}
for (const opt of ['--max-children', '--out', '--graph']) {
  assert(optsOf('tree').includes(opt), `tree should have ${opt}`);
}
for (const opt of ['--out', '--max-nodes', '--format', '--graph']) {
  assert(optsOf('wiki').includes(opt), `wiki should have ${opt}`);
}
assert(optsOf('prs').includes('--conflicts'), 'prs should have --conflicts');
assert(optsOf('status').includes('--graph'), 'status should have --graph');

// The query family carries --json so callers get machine-readable results
// (counts, cursors, build provenance) instead of parsing prose.
for (const cmd of ['query', 'map', 'explain', 'path', 'affected', 'stats', 'status']) {
  assert(optsOf(cmd).includes('--json'), `${cmd} should have --json`);
}
// MCP-parity commands: god_nodes / list_communities / get_neighbors.
for (const opt of ['--graph', '--json']) {
  assert(optsOf('god-nodes').includes(opt), `god-nodes should have ${opt}`);
  assert(optsOf('communities').includes(opt), `communities should have ${opt}`);
  assert(optsOf('neighbors').includes(opt), `neighbors should have ${opt}`);
}
for (const opt of ['--relation', '--graph']) {
  assert(optsOf('neighbors').includes(opt), `neighbors should have ${opt}`);
}

// Test 4: the compiled entrypoint parses --help (catches duplicate-flag
// registration and native-loading regressions that source imports mask)
const entry = join(__dirname, '..', '..', 'dist', 'index.js');
if (existsSync(entry)) {
  try {
    const help = execFileSync('node', [entry, '--help'], {
      stdio: 'pipe',
      encoding: 'utf-8',
    });
    assert(help.includes('Usage:'), 'dist entrypoint --help should print usage');
  } catch (e: any) {
    assert(false, `dist entrypoint should load: ${String(e.message).slice(0, 140)}`);
  }
} else {
  console.log('(dist not built - skipping entrypoint load check)');
}

// Test 5: the --budget default comes from the shared constant in
// src/defaults.ts (kept in sync with DEFAULT_QUERY_BUDGET in
// crates/astria-mcp/src/lib.rs) — not a hardcoded literal that can drift
// from the MCP server's default.
function budgetDefault(commandName: string): unknown {
  const command = program.commands.find((c: Command) => c.name() === commandName);
  return command?.options.find((o) => o.long === '--budget')?.defaultValue;
}
for (const name of ['query', 'map']) {
  assert(
    budgetDefault(name) === String(DEFAULT_QUERY_BUDGET),
    `${name} --budget default should be String(DEFAULT_QUERY_BUDGET), got ${budgetDefault(name)}`,
  );
}

// Summary
console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
  process.exit(1);
}
