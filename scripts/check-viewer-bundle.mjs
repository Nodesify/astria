#!/usr/bin/env node
// Viewer bundle drift guard: rebuilds the viewer from source with the same
// esbuild options as `packages/viewer`'s build script into a temp file and
// byte-compares the result with the committed bundle the Rust exporter
// embeds (`crates/astria-export/assets/viewer.js`, via include_str!).
// Source edits without a rebuild ship stale interactive HTML exports; this
// check fails CI instead.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const viewerDir = join(root, 'packages', 'viewer');
const committedBundle = join(root, 'crates', 'astria-export', 'assets', 'viewer.js');

// Same options as packages/viewer's build script — keep the two in sync.
const ESBUILD_ARGS = [
  'src/viewer.ts',
  '--bundle',
  '--minify',
  '--platform=browser',
  '--target=es2020',
];

function resolveEsbuild() {
  // Run esbuild's JS entry through node directly — spawning .cmd shims via
  // execFileSync is blocked on Windows. The workspace hoists deps to the
  // repo root, so both install locations are probed.
  const candidates = [
    join(viewerDir, 'node_modules', 'esbuild', 'bin', 'esbuild'),
    join(root, 'node_modules', 'esbuild', 'bin', 'esbuild'),
  ];
  for (const local of candidates) {
    try {
      execFileSync(process.execPath, [local, '--version'], { stdio: 'pipe' });
      return { cmd: process.execPath, args: [local] };
    } catch {
      // try the next location
    }
  }
  const npx = process.platform === 'win32' ? 'npx.cmd' : 'npx';
  return { cmd: npx, args: ['esbuild'] };
}

const tmp = mkdtempSync(join(tmpdir(), 'astria-viewer-check-'));
let failures = 0;
try {
  const { cmd, args } = resolveEsbuild();
  const rebuilt = join(tmp, 'viewer.js');
  execFileSync(cmd, [...args, ...ESBUILD_ARGS, `--outfile=${rebuilt}`], {
    cwd: viewerDir,
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  const committed = readFileSync(committedBundle, 'utf8');
  const fresh = readFileSync(rebuilt, 'utf8');
  if (committed !== fresh) {
    console.error(
      'viewer bundle drift: crates/astria-export/assets/viewer.js does not match a fresh build of packages/viewer/src/viewer.ts\n' +
        '  rebuild with: cd packages/viewer && npm run build',
    );
    failures = 1;
  } else {
    console.log('viewer bundle in sync with source');
  }
} catch (err) {
  console.error(`viewer bundle check could not run: ${err.message}`);
  failures = 1;
} finally {
  rmSync(tmp, { recursive: true, force: true });
}

process.exit(failures);
