// Verify distribution contents and native loading outside the source checkout.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const main = path.join(repo, 'packages', 'astria-cli');
const platform = process.argv[2];
if (!platform || !/^(win32-(x64|arm64)-msvc|darwin-(x64|arm64)|linux-(x64|arm64)-gnu|linux-x64-musl)$/.test(platform)) throw new Error('Pass a supported platform suffix.');
const platformDir = path.join(main, 'npm', platform);
const scratch = mkdtempSync(path.join(tmpdir(), 'astria-package-'));
const npmArgs = process.platform === 'win32' ? [process.env.npm_execpath || path.join(path.dirname(process.execPath), 'node_modules', 'npm', 'bin', 'npm-cli.js')] : [];
const npm = process.platform === 'win32' ? process.execPath : 'npm';
function run(command, args, cwd, env = process.env) {
  const result = spawnSync(command, args, { cwd, env, encoding: 'utf8', timeout: 120000 });
  if (result.error || result.status !== 0) throw new Error(`${command} ${args.join(' ')} failed: ${result.error?.message ?? result.stderr + result.stdout}`);
  return result.stdout;
}
try {
  const pkg = JSON.parse(readFileSync(path.join(main, 'package.json')));
  const nativePkg = JSON.parse(readFileSync(path.join(platformDir, 'package.json')));
  if (nativePkg.version !== pkg.version || pkg.optionalDependencies[nativePkg.name] !== pkg.version) throw new Error('Native package version/dependency does not match the CLI.');
  for (const dir of [main, platformDir]) {
    const packed = JSON.parse(run(npm, [...npmArgs, 'pack', '--ignore-scripts', '--json', '--pack-destination', scratch], dir))[0];
    if (dir === main && packed.files.some(f => f.path.includes('__tests__') || f.path.endsWith('.node') || f.path.endsWith('.dll'))) throw new Error('CLI tarball includes test or native build artifacts.');
    if (dir === platformDir && !packed.files.some(f => f.path.endsWith('.node'))) throw new Error('Platform tarball contains no native binary.');
  }
  const clean = path.join(scratch, 'install');
  mkdirSync(clean);
  writeFileSync(path.join(clean, 'package.json'), JSON.stringify({ private: true }));
  const tarballs = readdirSync(scratch).filter(f => f.endsWith('.tgz')).map(f => path.join(scratch, f));
  // prefer-offline, not offline: the packed tarballs under test are local files, but
  // the CLI's declared `commander` dependency must still resolve against the registry.
  // `npm ci` fills the tarball cache without caching packuments, so a strict --offline
  // install fails with ENOTCACHED in CI (and always would in the musl container,
  // whose npm cache starts empty).
  run(npm, [...npmArgs, 'install', '--prefer-offline', '--ignore-scripts', '--no-audit', '--no-fund', ...tarballs], clean);
  const fixture = path.join(clean, 'fixture');
  mkdirSync(fixture);
  writeFileSync(path.join(fixture, 'example.ts'), 'export function greet(name: string) { return `Hello ${name}`; }\n');
  const cli = path.join(clean, 'node_modules', '@nodesify', 'astria', 'dist', 'index.js');
  // Exclude checkout/cached native-runtime directories; only installed package and system PATH.
  const cleanPath = (process.env.PATH ?? '').split(path.delimiter).filter(p => !/ort\.pyke|nodesify-graphify|target[\\/]ort-link/i.test(p));
  const env = { ...process.env, PATH: [path.join(clean, 'node_modules', '.bin'), ...cleanPath].join(path.delimiter), ASTRIA_LLM_BACKEND: 'none' };
  run(process.execPath, [cli, 'run', fixture, '--backend', 'none'], clean, env);
  if (!existsSync(path.join(fixture, '.astria', 'db.sqlite'))) throw new Error('Packed CLI did not publish a graph.');
  const report = JSON.parse(run(process.execPath, [cli, 'doctor', '--graph', fixture, '--json'], clean, env));
  if (!report.ok || !report.checks.some(c => c.name === 'native' && c.status === 'ok')) throw new Error('Packed doctor failed native/runtime checks.');
  const map = run(process.execPath, [cli, 'map', '--graph', fixture], clean, env);
  if (!map.includes('greet')) throw new Error('Packed CLI could not retrieve the fixture symbol.');
  console.log(`Packed installation verified: ${platform} @ ${pkg.version}`);
} finally {
  // Only the exact directory created by this invocation can be removed.
  if (path.dirname(scratch) === path.resolve(tmpdir()) && path.basename(scratch).startsWith('astria-package-')) rmSync(scratch, { recursive: true, force: true });
}
