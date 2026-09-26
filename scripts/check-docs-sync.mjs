// Docs-vs-code drift guard: fails when documented claims fall behind the code.
// Added after ARCHITECTURE.md missed astria-embed entirely and documented a
// relationship set ("Defines") that does not exist in the code.
//
// Zero runtime dependencies. Run: node scripts/check-docs-sync.mjs
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const problems = [];

const read = (p) => readFileSync(path.join(repoRoot, p), 'utf8');

// 1. Every workspace crate is documented in ARCHITECTURE.md's crate table.
const cargo = read('Cargo.toml');
const members = [...new Set([...cargo.matchAll(/"crates\/([\w-]+)"/g)].map((m) => m[1]))];
const arch = read('ARCHITECTURE.md');
for (const name of members) {
  if (!arch.includes(`\`${name}\``)) {
    problems.push(`ARCHITECTURE.md: crate \`${name}\` (workspace member) is not documented`);
  }
}

// 2. Every relation literal the code emits is documented in ARCHITECTURE.md's
//    relationship section. Scan the files that construct edges.
const relationFiles = [
  'crates/astria-extract/src/walkers.rs',
  'crates/astria-extract/src/docs.rs',
  'crates/astria-extract/src/manifest.rs',
  'crates/astria-build/src/hyperedges.rs',
  'crates/astria-semantic/src/lib.rs',
  'crates/astria-ingest/src/lib.rs',
  'crates/astria-ingest/src/scip.rs',
  'crates/astria-embed/src/lib.rs',
  'crates/astria-query/src/lib.rs',
];
const relations = new Set();
const patterns = [
  /relation:\s*"([a-z_]+)"/g,
  /relation:\s*relation\.into\(\)/g, // dynamic — skip, but note the pattern exists
];
for (const file of relationFiles) {
  let src;
  try {
    src = read(file);
  } catch {
    continue; // feature-gated or renamed file
  }
  for (const m of src.matchAll(/relation:\s*"([a-z_]+)"/g)) relations.add(m[1]);
  // semantic allowlist: const ALLOWED_RELATIONS: &[&str] = &["a", "b", ...]
  const allow = src.match(/ALLOWED_RELATIONS[^=]*=\s*&\[([^\]]*)\]/);
  if (allow) {
    for (const m of allow[1].matchAll(/"([a-z_]+)"/g)) relations.add(m[1]);
  }
}
const relSection = arch.slice(
  arch.indexOf('### Relationship Types'),
  arch.indexOf('### Relationship Types') < 0
    ? undefined
    : arch.indexOf('\n## ', arch.indexOf('### Relationship Types')),
);
if (relSection) {
  for (const rel of [...relations].sort()) {
    if (!relSection.includes(`\`${rel}\``)) {
      problems.push(
        `ARCHITECTURE.md: relation \`${rel}\` is emitted by the code but not documented in the relationship section`,
      );
    }
  }
} else {
  problems.push('ARCHITECTURE.md: "### Relationship Types" section not found');
}

// 3. MSRV: the workspace rust-version must be what README and CONTRIBUTING claim.
const rustVersion = cargo.match(/\[workspace\.package\][^[]*?rust-version\s*=\s*"([\d.]+)"/s)?.[1];
if (!rustVersion) {
  problems.push('Cargo.toml: [workspace.package] rust-version is not declared');
} else {
  const readme = read('README.md');
  const contributing = read('CONTRIBUTING.md');
  if (!readme.includes(`Rust ${rustVersion}`)) {
    problems.push(`README.md: does not state Rust ${rustVersion}+ (workspace rust-version)`);
  }
  if (!contributing.includes(`Rust** ${rustVersion}`) && !contributing.includes(`Rust ${rustVersion}`)) {
    problems.push(`CONTRIBUTING.md: does not state Rust ${rustVersion}+ (workspace rust-version)`);
  }
}

// 4. Node version: badge, engines, and prose must agree with the CI matrix.
const pkg = JSON.parse(read('packages/astria-cli/package.json'));
const enginesNode = pkg.engines?.node ?? '';
const enginesMajor = Number(enginesNode.replace(/[^\d]/g, ''));
const readme = read('README.md');
const badgeMajor = readme.match(/badge\/node-(\d+)-/)?.[1];
const ci = read('.github/workflows/ci.yml');
const ciMajors = [...ci.matchAll(/node-version:\s*'(\d+)'/g)].map((m) => m[1]);
for (const v of ciMajors) {
  if (v !== badgeMajor) problems.push(`README badge says Node ${badgeMajor} but CI tests Node ${v}`);
}
if (enginesMajor && badgeMajor && enginesMajor > Number(badgeMajor)) {
  problems.push(`engines requires Node ${enginesMajor}+ but the badge advertises ${badgeMajor}`);
}
if (!readme.includes(`Node.js >= ${badgeMajor}`) && badgeMajor) {
  problems.push(`README.md: prose does not state Node.js >= ${badgeMajor}`);
}

if (problems.length) {
  console.error('docs drift detected:');
  for (const p of problems) console.error(`  - ${p}`);
  process.exit(1);
}
console.log(
  `docs in sync: ${members.length} crates, ${relations.size} relations, rust-version ${rustVersion}, node ${badgeMajor}`,
);
