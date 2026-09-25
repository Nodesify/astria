#!/usr/bin/env node
// Guards against @napi-rs/cli drift (#48, #49): the release workflow speaks
// napi v2 (`npx napi create-npm-dir`), so the declared range in
// packages/graphify-cli/package.json and the version resolved in
// package-lock.json must both be 2.x. Caret ranges drift; majors silently
// change. v2-only on purpose — widen here if release.yml ever moves to v3.
import { readFileSync, createReadStream } from 'node:fs';
import { createInterface } from 'node:readline';

const declared = JSON.parse(
  readFileSync('packages/graphify-cli/package.json', 'utf8'),
).devDependencies['@napi-rs/cli'];

// package-lock.json is megabytes — stream it instead of JSON.parse.
// Matches the entry under either install layout (note: no leading quote —
// the nested key is "packages/graphify-cli/node_modules/@napi-rs/cli"):
//   "packages/graphify-cli/node_modules/@napi-rs/cli": { ... }
//   "node_modules/@napi-rs/cli": { ... }
const stream = createReadStream('package-lock.json', 'utf8');
const lockLines = createInterface({ input: stream });
let resolved = null;
let inEntry = false;
for await (const line of lockLines) {
  if (line.includes('node_modules/@napi-rs/cli": {')) {
    inEntry = true;
  } else if (inEntry) {
    const match = line.match(/"version": "([^"]+)"/);
    if (match) {
      resolved = match[1];
      break;
    }
  }
}
stream.destroy();

const declaredIsV2 = /^(\^)?2\./.test(declared ?? '');
const resolvedIsV2 = /^2\./.test(resolved ?? '');
if (!declaredIsV2 || !resolvedIsV2) {
  console.error(
    `@napi-rs/cli version drift: declared ${JSON.stringify(declared ?? null)}` +
      ` in packages/graphify-cli/package.json vs resolved ${JSON.stringify(resolved ?? null)}` +
      ' in package-lock.json. The release workflow speaks napi v2; both must be 2.x.',
  );
  process.exit(1);
}
