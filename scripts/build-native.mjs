// Canonical local native-module build, used by `npm run napi:build`.
// Mirrors CI (ci.yml): cargo build --release --locked -p astria-napi, then
// copy the platform library into dist/astria.node. The @napi-rs CLI cannot
// drive this build (see scripts/check-napi-version.mjs for the drift guard),
// so the script shells out to cargo directly.
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const sleep = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

const pkgDir = path.resolve(fileURLToPath(import.meta.url), '..', '..', 'packages', 'astria-cli');
const repoRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url))); // scripts/.. = repo root
const distDir = path.join(pkgDir, 'dist');
const outFile = path.join(distDir, 'astria.node');

const debug = process.argv.includes('--debug');
const cargoArgs = ['build', '--locked', '-p', 'astria-napi'];
if (!debug) cargoArgs.push('--release');

console.log(`$ cargo ${cargoArgs.join(' ')}`);
const run = spawnSync('cargo', cargoArgs, { cwd: repoRoot, stdio: 'inherit' });
if (run.status !== 0) {
  console.error(`cargo build failed with status ${run.status}`);
  process.exit(run.status ?? 1);
}

// The just-built profile wins: with --debug a stale target/release artifact
// must not shadow the fresh target/debug build (it always did before).
const profiles = debug ? ['debug', 'release'] : ['release', 'debug'];
const libNames = ['astria_napi.dll', 'libastria_napi.so', 'libastria_napi.dylib'];
const candidates = profiles.flatMap((profile) => libNames.map((name) => `target/${profile}/${name}`));
const lib = candidates
  .map((rel) => path.join(repoRoot, rel))
  .find((p) => existsSync(p));

if (!lib) {
  console.error(`built library not found; looked in ${repoRoot}\\target for ${candidates.length} names`);
  process.exit(1);
}

mkdirSync(distDir, { recursive: true });
const copyWithRetry = (src, dest, attempts = 5) => {
  for (let i = 0; i < attempts; i++) {
    try {
      copyFileSync(src, dest);
      return true;
    } catch (e) {
      if (i === attempts - 1) return e;
      process.stderr.write(`copy blocked (retry ${i + 1}/${attempts}): ${e.code}\n`);
      sleep(3000);
    }
  }
};
const copied = copyWithRetry(lib, outFile);
if (copied !== true) {
  // A locked destination means a running astria process (CLI, MCP server)
  // has the old binary mapped. Ship the fresh build next to it instead of
  // failing the whole build, and say exactly how to finish the swap.
  const fallback = `${outFile}.new`;
  copyFileSync(lib, fallback);
  console.warn(
    `warning: ${outFile} is locked (a running astria process has it loaded); ` +
    `wrote the fresh build to ${fallback}.\n` +
    `Stop the astria process, then replace the file: mv "${fallback}" "${outFile}"`,
  );
} else {
  console.log(`copied ${path.relative(repoRoot, lib)} -> ${path.relative(repoRoot, outFile)}`);
}
