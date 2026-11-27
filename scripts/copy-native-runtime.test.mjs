// Tests for copy-native-runtime.mjs: node --test "scripts/*.test.mjs"
//
// The collector only runs inside the release build, where a failure costs a
// 30-minute matrix — so its reading of Cargo's build records is exercised
// here against both of Cargo's directory layouts.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, rmSync, statSync, utimesSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('./copy-native-runtime.mjs', import.meta.url));

function sandbox(t) {
  const dir = mkdtempSync(path.join(tmpdir(), 'astria-collect-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const build = path.join(dir, 'build');
  const dist = path.join(dir, 'dist');
  mkdirSync(build);
  return { dir, build, dist };
}

// A runtime directory holding a stub DirectML.dll with the given bytes.
function runtime(dir, name, bytes) {
  const linkDir = path.join(dir, name);
  mkdirSync(linkDir);
  writeFileSync(path.join(linkDir, 'DirectML.dll'), bytes);
  return linkDir;
}

// One ort-sys build-script record, in the layout of the given Cargo line.
// `directml: false` is what a build without the embedding feature records.
function record(build, layout, hash, linkDir, { directml = true } = {}) {
  const lines = [
    'cargo:rerun-if-env-changed=ORT_LIB_LOCATION',
    'cargo:rustc-link-lib=dxguid',
    ...(directml ? ['cargo:rustc-link-lib=DirectML'] : []),
    `cargo:rustc-link-search=native=${linkDir}`,
    'cargo:rustc-link-lib=static=onnxruntime',
  ].join('\n') + '\n';
  const file =
    layout === '1.100+'
      ? path.join(build, 'ort-sys', hash, 'run', 'stdout')
      : path.join(build, `ort-sys-${hash}`, 'output');
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, lines);
  return file;
}

function collect(build, dist) {
  return spawnSync(process.execPath, [script, build, dist], { encoding: 'utf8' });
}

test('reads the Cargo 1.100+ layout: build/ort-sys/<hash>/run/stdout', (t) => {
  const { build, dist } = sandbox(t);
  const linkDir = runtime(path.join(build, '..'), 'cache', 'runtime-1');
  record(build, '1.100+', 'd3802f322f74752d', linkDir);
  const run = collect(build, dist);
  assert.equal(run.status, 0, run.stderr);
  assert.equal(readFileSync(path.join(dist, 'DirectML.dll'), 'utf8'), 'runtime-1');
});

test('reads the layout up to Cargo 1.99: build/ort-sys-<hash>/output', (t) => {
  const { build, dist } = sandbox(t);
  const linkDir = runtime(path.join(build, '..'), 'cache', 'runtime-1');
  record(build, '1.99', '01d36010fa8e8378', linkDir);
  const run = collect(build, dist);
  assert.equal(run.status, 0, run.stderr);
  assert.equal(readFileSync(path.join(dist, 'DirectML.dll'), 'utf8'), 'runtime-1');
});

test('takes the most recently written record', (t) => {
  const { build, dist } = sandbox(t);
  const base = path.join(build, '..');
  const older = record(build, '1.100+', '1111111111111111', runtime(base, 'cache-a', 'older'));
  const newer = record(build, '1.100+', '2222222222222222', runtime(base, 'cache-b', 'newer'));
  const past = new Date(Date.now() - 60_000);
  utimesSync(older, past, past);
  utimesSync(newer, new Date(), new Date());
  const run = collect(build, dist);
  assert.equal(run.status, 0, run.stderr);
  assert.equal(readFileSync(path.join(dist, 'DirectML.dll'), 'utf8'), 'newer');
});

test('skips records of builds without the embedding runtime', (t) => {
  const { build, dist } = sandbox(t);
  const base = path.join(build, '..');
  // Newest first on disk order: a CPU-only record sits beside the DirectML one.
  const cpu = record(build, '1.100+', '3333333333333333', runtime(base, 'cpu-cache', 'cpu'), { directml: false });
  const withRuntime = record(build, '1.99', '4444444444444444', runtime(base, 'dml-cache', 'directml'));
  utimesSync(cpu, new Date(), new Date());
  utimesSync(withRuntime, new Date(Date.now() - 60_000), new Date(Date.now() - 60_000));
  const run = collect(build, dist);
  assert.equal(run.status, 0, run.stderr);
  assert.equal(readFileSync(path.join(dist, 'DirectML.dll'), 'utf8'), 'directml');
});

test('leaves an identical runtime already in place untouched', (t) => {
  const { build, dist } = sandbox(t);
  mkdirSync(dist);
  const target = path.join(dist, 'DirectML.dll');
  writeFileSync(target, 'same');
  const past = new Date(Date.now() - 60_000);
  utimesSync(target, past, past);
  const before = statSync(target).mtimeMs;
  record(build, '1.100+', '5555555555555555', runtime(path.join(build, '..'), 'cache', 'same'));
  const run = collect(build, dist);
  assert.equal(run.status, 0, run.stderr);
  assert.equal(statSync(target).mtimeMs, before);
  assert.equal(readFileSync(target, 'utf8'), 'same');
});

test('fails loudly when no link-search directory holds the runtime', (t) => {
  const { build, dist } = sandbox(t);
  const empty = path.join(build, '..', 'cache', 'empty');
  mkdirSync(empty, { recursive: true });
  record(build, '1.100+', '6666666666666666', empty);
  const run = collect(build, dist);
  assert.notEqual(run.status, 0);
  assert.match(run.stderr, /DirectML\.dll/);
  assert.equal(existsSync(path.join(dist, 'DirectML.dll')), false);
});

test('fails loudly when there is no ort-sys record at all', (t) => {
  const { build, dist } = sandbox(t);
  const run = collect(build, dist);
  assert.notEqual(run.status, 0);
  assert.match(run.stderr, /DirectML\.dll/);
});
