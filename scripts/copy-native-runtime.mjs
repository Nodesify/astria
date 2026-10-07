// Collect the DirectML runtime from the exact link-search paths recorded by Cargo.
import { readdirSync, readFileSync, existsSync, statSync, copyFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
const [buildDirectory, destination] = process.argv.slice(2);
if (!buildDirectory || !destination) throw new Error('Pass Cargo build directory and runtime destination.');
const outputs = readdirSync(buildDirectory).filter(n => n.startsWith('ort-sys-'))
  .map(n => path.join(buildDirectory, n, 'output')).filter(existsSync)
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
if (!runtime) throw new Error('The Windows embedding build requires DirectML.dll, but its Cargo link-search directories contain no runtime.');
mkdirSync(destination, { recursive: true });
copyFileSync(runtime, path.join(destination, 'DirectML.dll'));
console.log('Collected DirectML.dll from the ONNX build cache.');
