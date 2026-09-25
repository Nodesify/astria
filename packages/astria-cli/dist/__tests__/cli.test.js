"use strict";
/**
 * CLI test — validates the REAL Commander program from src/index.ts:
 * every command registered, expected options present, and the version
 * in sync with package.json. Does not execute any command actions.
 *
 * Run with: npx tsx src/__tests__/cli.test.ts
 */
Object.defineProperty(exports, "__esModule", { value: true });
const child_process_1 = require("child_process");
const fs_1 = require("fs");
const path_1 = require("path");
const index_1 = require("../index");
// eslint-disable-next-line @typescript-eslint/no-var-requires
const pkg = require('../../package.json');
let passed = 0;
let failed = 0;
function assert(condition, message) {
    if (condition) {
        passed++;
    }
    else {
        failed++;
        console.error(`FAIL: ${message}`);
    }
}
// ---- The real program ----
// Importing index.ts registers every command AND loads the native binding,
// so this test also fails fast when the native module is broken.
// Test 1: every command the CLI ships is registered
const commandNames = index_1.program.commands.map((c) => c.name());
const allCommands = [
    'run', 'update', 'watch', 'explain', 'query', 'path', 'map', 'affected',
    'stats', 'export', 'cluster-only', 'merge', 'diff', 'history', 'mcp',
    'tree', 'wiki', 'prs', 'add', 'status', 'install', 'uninstall', 'hook',
];
for (const cmd of allCommands) {
    assert(commandNames.includes(cmd), `Command "${cmd}" should be registered`);
}
// Test 2: version stays in sync with package.json (the stub test used to
// hard-code 0.1.0 while the package moved on to 0.5.0)
assert(index_1.program.version() === pkg.version, `Program version (${index_1.program.version()}) should match package.json (${pkg.version})`);
// Test 3: query carries its full option surface (--directed/--detail/--cursor
// were added in 0.5.0; a mirror of the program cannot see them)
function optsOf(name) {
    const cmd = index_1.program.commands.find((c) => c.name() === name);
    assert(cmd !== undefined, `Command "${name}" should exist for option check`);
    return cmd ? cmd.options.map((o) => o.long) : [];
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
const formatOpt = index_1.program.commands
    .find((c) => c.name() === 'export')
    ?.options.find((o) => o.long === '--format');
assert(!!formatOpt && (formatOpt.defaultValue ?? 'json') === 'json', 'export --format should default to "json"');
const modeOpt = index_1.program.commands
    .find((c) => c.name() === 'export')
    ?.options.find((o) => o.long === '--mode');
assert(!!modeOpt && (modeOpt.defaultValue ?? 'standard') === 'standard', 'export --mode should default to "standard"');
// Test 3b: napi platform binaries are shipped via optionalDependencies — a
// stale pin makes npm install a previous version's .node binary (0.6.0
// shipped 0.5.0's binary because the pins were not bumped)
const optDeps = pkg.optionalDependencies ?? {};
assert(Object.keys(optDeps).length === 5, 'all 5 napi platform packages should be pinned');
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
// Test 4: the compiled entrypoint parses --help (catches duplicate-flag
// registration and native-loading regressions that source imports mask)
const entry = (0, path_1.join)(__dirname, '..', '..', 'dist', 'index.js');
if ((0, fs_1.existsSync)(entry)) {
    try {
        const help = (0, child_process_1.execFileSync)('node', [entry, '--help'], {
            stdio: 'pipe',
            encoding: 'utf-8',
        });
        assert(help.includes('Usage:'), 'dist entrypoint --help should print usage');
    }
    catch (e) {
        assert(false, `dist entrypoint should load: ${String(e.message).slice(0, 140)}`);
    }
}
else {
    console.log('(dist not built - skipping entrypoint load check)');
}
// Summary
console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
    process.exit(1);
}
//# sourceMappingURL=cli.test.js.map