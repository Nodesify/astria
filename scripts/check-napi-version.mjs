#!/usr/bin/env node
// Guards against @napi-rs/cli drift (#48, #49): the release workflow drives
// the `napi` CLI from the lockfile (`npx napi create-npm-dirs`), so the
// declared range in packages/astria-cli/package.json and the version resolved
// in package-lock.json must agree on the major. Caret ranges drift; majors
// silently change. v3 is accepted because release.yml moved to the v3-only
// `create-npm-dirs` command (v2's `create-npm-dir -t .` no longer parses) and
// the napi config was migrated from `triples` to flat `targets` — v2's
// `triples.defaults` + `triples.additional` expansion contained a duplicate
// target, which v3 rejects outright. Add majors here deliberately.
import { readFileSync, createReadStream } from 'node:fs';
import { createInterface } from 'node:readline';

const ALLOWED_MAJORS = new Set(['2', '3']);

const declared = JSON.parse(
  readFileSync('packages/astria-cli/package.json', 'utf8'),
).devDependencies['@napi-rs/cli'];

// package-lock.json is megabytes — stream it instead of JSON.parse.
// Matches the entry under either install layout (note: no leading quote —
// the nested key is "packages/astria-cli/node_modules/@napi-rs/cli"):
//   "packages/astria-cli/node_modules/@napi-rs/cli": { ... }
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

// Strip a leading range operator, then compare majors. Both the declared
// range and the resolved version must sit on the same allowed major so a
// range like `^3` can never silently resolve to a major we have not migrated
// the release workflow for.
const declaredMajor = (declared ?? '').replace(/^[\^~]/, '').split('.')[0];
const resolvedMajor = (resolved ?? '').split('.')[0];
const majorsAgree = declaredMajor !== '' && declaredMajor === resolvedMajor;
if (!ALLOWED_MAJORS.has(declaredMajor) || !majorsAgree) {
  console.error(
    `@napi-rs/cli version drift: declared ${JSON.stringify(declared ?? null)}` +
      ` in packages/astria-cli/package.json vs resolved ${JSON.stringify(resolved ?? null)}` +
      ` in package-lock.json. Both must sit on the same major, one of` +
      ` ${[...ALLOWED_MAJORS].join('/')} (the majors the release workflow is migrated for).`,
  );
  process.exit(1);
}
