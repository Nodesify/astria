// napi generates a files allowlist containing only the .node file; include runtime sidecars.
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'packages', 'astria-cli', 'npm');
for (const entry of readdirSync(root, { withFileTypes: true })) {
  if (!entry.isDirectory()) continue;
  const dir = path.join(root, entry.name);
  const file = path.join(dir, 'package.json');
  const pkg = JSON.parse(readFileSync(file, 'utf8'));
  const sidecars = readdirSync(dir).filter(n => n.endsWith('.dll'));
  pkg.files = [...new Set([...pkg.files, ...sidecars])];
  writeFileSync(file, JSON.stringify(pkg, null, 2) + '\n');
}
