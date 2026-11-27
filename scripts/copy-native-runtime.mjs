// Collect the DirectML runtime from the exact link-search paths recorded by Cargo.
import { readdirSync, readFileSync, existsSync, statSync, copyFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
const [buildDirectory, destination] = process.argv.slice(2);
if (!buildDirectory || !destination) throw new Error('Pass Cargo build directory and runtime destination.');
// A build script's stdout is where its `cargo:rustc-link-*` lines are recorded.
// Cargo keeps it in `build/<crate>-<hash>/output` up to 1.99, and in
// `build/<crate>/<hash>/run/stdout` from 1.100 on; read whichever is there.
const outputs = readdirSync(buildDirectory, { withFileTypes: true })
  .filter(entry => entry.isDirectory() && (entry.name.startsWith('ort-sys-') || entry.name === 'ort-sys'))
  .flatMap(entry => {
    const dir = path.join(buildDirectory, entry.name);
    return entry.name === 'ort-sys'
      ? readdirSync(dir, { withFileTypes: true })
          .filter(hash => hash.isDirectory())
          .map(hash => path.join(dir, hash.name, 'run', 'stdout'))
      : [path.join(dir, 'output')];
  })
  .filter(existsSync)
  .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
let runtime;
for (const output of outputs) {
  const text = readFileSync(output, 'utf8');
  if (!/^cargo:rustc-link-lib=DirectML$/m.test(text.replace(/\r/g, ''))) continue;
  for (const match of text.matchAll(/^cargo:rustc-link-search=native=(.+)$/gm)) {
    const file = path.join(match[1].trim(), 'DirectML.dll');
    if (existsSync(file)) { runtime = file; break; }
  }
  if (runtime) break;
}
if (!runtime) {
  throw new Error(`The Windows embedding build requires DirectML.dll, but its Cargo link-search directories contain no runtime (${outputs.length} ort-sys build record${outputs.length === 1 ? '' : 's'} checked under ${buildDirectory}).`);
}
mkdirSync(destination, { recursive: true });
const target = path.join(destination, 'DirectML.dll');
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
// An already-loaded identical runtime needs no replacement on Windows.
if (!existsSync(target) || digest(runtime) !== digest(target)) copyFileSync(runtime, target);
console.log('Collected DirectML.dll from the ONNX build cache.');
