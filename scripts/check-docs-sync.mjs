// Docs-vs-code drift guard: fails when documented claims fall behind the code.
// Added after ARCHITECTURE.md missed astria-embed entirely and documented a
// relationship set ("Defines") that does not exist in the code.
//
// Checks (both directions where marked):
//   1. every workspace crate documented in ARCHITECTURE.md
//   2. relations: every emitted relation documented, AND every documented
//      relation emitted or present in production code (or marked external-only)
//   3. MSRV and Node version claims match workspace/CI metadata
//   4. generated language-support table is fresh
//   5. generated SQLite schema block is fresh
//   6. env vars: every ASTRIA_* read is documented, every documented one is read
//   7. every registered CLI command word and long flag appears in cli.md
//   8. every registered MCP tool name appears in mcp-tools.md
//
// Zero runtime dependencies. Run: node scripts/check-docs-sync.mjs
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { generateLanguageSupport, languageSupportPath } from './generate-language-support.mjs';
import {
  generateSchemaDocs,
  schemaDocsPath,
  SCHEMA_BEGIN,
  SCHEMA_END,
} from './generate-schema-docs.mjs';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const problems = [];

const read = (p) => readFileSync(path.join(repoRoot, p), 'utf8').replace(/\r\n/g, '\n');

// 1. Every workspace crate is documented in ARCHITECTURE.md's crate table.
const cargo = read('Cargo.toml');
const members = [...new Set([...cargo.matchAll(/"crates\/([\w-]+)"/g)].map((m) => m[1]))];
const arch = read('ARCHITECTURE.md');
for (const name of members) {
  if (!arch.includes(`\`${name}\``)) {
    problems.push(`ARCHITECTURE.md: crate \`${name}\` (workspace member) is not documented`);
  }
}

// 1b. Counted claims must match the manifests: "N crates" equals workspace
// members, "N registered language configurations"/"N languages" equals the
// language registry macro arms. These numbers rot silently otherwise.
const languageRegistry = read('crates/astria-core/src/languages.rs');
const languageCount = (languageRegistry.match(/^\s*[A-Z][A-Za-z0-9]*, "/gm) || []).length;
const countClaims = [
  ['ARCHITECTURE.md', /(\d+) domain-specific crates/, members.length, 'domain-specific crates'],
  ['README.md', /Rust workspace with (\d+) crates/, members.length, 'crates'],
  ['ARCHITECTURE.md', /Uses (\d+) registered language configurations/, languageCount, 'registered language configurations'],
  ['website/docs/explanation/architecture.md', /Uses (\d+) registered language configurations/, languageCount, 'registered language configurations'],
  ['README.md', /Uses (\d+) registered language configurations/, languageCount, 'registered language configurations'],
  ['ARCHITECTURE.md', /registry currently defines (\d+) language configurations/, languageCount, 'language configurations'],
];
for (const [file, re, expected, label] of countClaims) {
  const m = read(file).match(re);
  if (!m) {
    problems.push(`${file}: ${label} claim not found (pattern ${re})`);
  } else if (Number(m[1]) !== expected) {
    problems.push(`${file}: claims ${m[1]} ${label}, manifest says ${expected}`);
  }
}

// 2. Every relation literal the code emits is documented in ARCHITECTURE.md's
//    relationship section. Scan the files that construct edges.
const relationFiles = [
  'crates/astria-extract/src/walkers.rs',
  'crates/astria-extract/src/docs.rs',
  'crates/astria-extract/src/manifest.rs',
  'crates/astria-build/src/crosslayer.rs',
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
// Test modules carry fixtures (e.g. the clamp-sample `relation: "forks"`) that
// are not emitters — strip `#[cfg(test)] mod … { … }` blocks before scanning.
// The brace matcher is string/comment/raw-string aware so JSON fixtures inside
// tests cannot unbalance it.
const stripTestModules = (src) => {
  let out = src;
  for (;;) {
    const attr = out.indexOf('#[cfg(test)]');
    if (attr < 0) break;
    const head = out.slice(attr, attr + 96);
    if (!/^#\[cfg\(test\)\]\s*(?:pub(?:\([\w ]+\))?\s+)?mod\s+\w+/.test(head)) {
      // attribute on a non-module item (e.g. a test-only static): drop the marker and move on
      out = out.slice(0, attr) + out.slice(attr + '#[cfg(test)]'.length);
      continue;
    }
    const open = out.indexOf('{', attr);
    if (open < 0) break;
    let depth = 0;
    let i = open;
    let closed = false;
    while (i < out.length) {
      const c = out[i];
      if (c === '/' && out[i + 1] === '/') {
        const e = out.indexOf('\n', i);
        i = e < 0 ? out.length : e;
        continue;
      }
      if (c === '/' && out[i + 1] === '*') {
        const e = out.indexOf('*/', i + 2);
        i = e < 0 ? out.length : e + 2;
        continue;
      }
      if (c === '"') {
        i++;
        while (i < out.length && out[i] !== '"') {
          if (out[i] === '\\') i++;
          i++;
        }
        i++;
        continue;
      }
      if (c === 'r' && (out[i + 1] === '"' || out[i + 1] === '#')) {
        let hashes = 0;
        let j = i + 1;
        while (out[j] === '#') {
          hashes++;
          j++;
        }
        if (out[j] === '"') {
          const needle = '"' + '#'.repeat(hashes);
          const e = out.indexOf(needle, j + 1);
          i = e < 0 ? out.length : e + needle.length;
          continue;
        }
      }
      if (c === "'") {
        if (out[i + 1] === '\\') i += 4;
        else if (out[i + 2] === "'") i += 3;
        else i++; // lifetime, not a char literal
        continue;
      }
      if (c === '{') depth++;
      if (c === '}') {
        depth--;
        if (depth === 0) {
          out = out.slice(0, attr) + out.slice(i + 1);
          closed = true;
          break;
        }
      }
      i++;
    }
    if (!closed) break; // unbalanced — leave the file unstripped rather than guess
  }
  return out;
};

for (const file of relationFiles) {
  let src;
  try {
    src = stripTestModules(read(file));
  } catch {
    continue; // feature-gated or renamed file
  }
  for (const m of src.matchAll(/relation:\s*"([a-z_]+)"/g)) relations.add(m[1]);
  // semantic allowlist: const ALLOWED_RELATIONS: &[&str] = &["a", "b", ...]
  const allow = src.match(/ALLOWED_RELATIONS[^=]*=\s*&\[([^\]]*)\]/);
  if (allow) {
    for (const m of allow[1].matchAll(/"([a-z_]+)"/g)) relations.add(m[1]);
  }
  // crosslayer allowlist: const EMITTED_RELATIONS: &[&str] = &["a", "b", ...]
  const emitted = src.match(/EMITTED_RELATIONS[^=]*=\s*&\[([^\]]*)\]/);
  if (emitted) {
    for (const m of emitted[1].matchAll(/"([a-z_]+)"/g)) relations.add(m[1]);
  }
}
const relSection = arch.slice(
  arch.indexOf('### Relationship Types'),
  arch.indexOf('### Relationship Types') < 0
    ? undefined
    : arch.indexOf('\n## ', arch.indexOf('### Relationship Types')),
);
if (!relSection) {
  problems.push('ARCHITECTURE.md: "### Relationship Types" section not found');
}
// The website graph-model page is the canonical user-facing relation table —
// it must document every emitted relation as well.
const graphModel = read('website/docs/reference/graph-model.md');
const gmSection = graphModel.slice(
  graphModel.indexOf('## Relations'),
  graphModel.indexOf('## Relations') < 0
    ? undefined
    : graphModel.indexOf('\n## ', graphModel.indexOf('## Relations')),
);
if (!gmSection) {
  problems.push('graph-model.md: "## Relations" section not found');
}
if (relSection) {
  for (const rel of [...relations].sort()) {
    if (!relSection.includes(`\`${rel}\``)) {
      problems.push(
        `ARCHITECTURE.md: relation \`${rel}\` is emitted by the code but not documented in the relationship section`,
      );
    }
    if (gmSection && !gmSection.includes(`\`${rel}\``)) {
      problems.push(
        `graph-model.md: relation \`${rel}\` is emitted by the code but missing from the relation table`,
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

// The language table is generated from the runtime registry and parser configs.
if (read(languageSupportPath).replace(/\r\n/g, '\n') !== generateLanguageSupport()) {
  problems.push(`${languageSupportPath}: stale; run node scripts/generate-language-support.mjs`);
}

// Shared corpus for the reverse checks below: all production Rust sources in
// crates/ (test modules stripped) and all TypeScript sources in the CLI.
const walkSources = (dir, ext, acc) => {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (['target', 'node_modules', 'assets', 'dist', '__tests__'].includes(entry.name)) continue;
      walkSources(path.join(dir, entry.name), ext, acc);
    } else if (entry.name.endsWith(ext)) {
      acc.push(path.join(dir, entry.name));
    }
  }
};
const rustSources = [];
walkSources(path.join(repoRoot, 'crates'), '.rs', rustSources);
const prodRust = rustSources.map((f) => stripTestModules(readFileSync(f, 'utf8'))).join('\n');
const tsSources = [];
walkSources(path.join(repoRoot, 'packages/astria-cli/src'), '.ts', tsSources);
const cliSources = tsSources.map((f) => readFileSync(f, 'utf8')).join('\n');

// 5. Reverse relation check: every relation documented in ARCHITECTURE.md's
//    relationship section must be emitted, appear as a literal somewhere in
//    production code (e.g. SQL strings, consumption allowlists), or sit on a
//    line explicitly marked "(external only)". This is the check that would
//    have caught the documented-but-never-emitted `method`/`inherits`/`forks`.
const RELATION_STOP_TOKENS = new Set(['crosslayer', 'deep', 'global', 'hub', 'llm']);
const documentedRelations = new Set();
for (const line of `${relSection}\n${gmSection}`.split('\n')) {
  if (line.includes('external only')) continue;
  for (const m of line.matchAll(/`([a-z_]{3,})`/g)) {
    if (!RELATION_STOP_TOKENS.has(m[1])) documentedRelations.add(m[1]);
  }
}
const codeHasLiteral = (rel) => prodRust.includes(`"${rel}"`) || prodRust.includes(`'${rel}'`);
for (const rel of [...documentedRelations].sort()) {
  if (!relations.has(rel) && !codeHasLiteral(rel)) {
    problems.push(
      `ARCHITECTURE.md: relation \`${rel}\` is documented but never emitted or referenced in production code ` +
        `(if it is intentionally external-only, mark the line "(external only)")`,
    );
  }
}

// 6. The generated SQLite schema block must be fresh.
const archSite = read(schemaDocsPath);
const schemaBlock = archSite.match(
  new RegExp(
    `${SCHEMA_BEGIN.replace(/[-/\\^$*+?.()|[\]{}]/g, '\\$&')}\\n([\\s\\S]*?)\\n${SCHEMA_END.replace(/[-/\\^$*+?.()|[\]{}]/g, '\\$&')}`,
  ),
);
if (!schemaBlock) {
  problems.push(`${schemaDocsPath}: generated schema block markers missing`);
} else if (schemaBlock[1] !== generateSchemaDocs()) {
  problems.push(`${schemaDocsPath}: stale schema block; run node scripts/generate-schema-docs.mjs`);
}

// 7. Env vars: every ASTRIA_* the code reads is documented in env-vars.md, and
//    every documented ASTRIA_*/third-party variable is actually read.
const envRead = new Set();
for (const m of prodRust.matchAll(/env_var\("([A-Z0-9_]+)"\)/g)) envRead.add(`ASTRIA_${m[1]}`);
// typed wrappers around env_var (astria-semantic jev.rs: env_bool/env_f64/...)
for (const m of prodRust.matchAll(/env_(?:bool|f64|u64|usize|str)\("([A-Z0-9_]+)"/g)) {
  envRead.add(`ASTRIA_${m[1]}`);
}
for (const m of prodRust.matchAll(/env::var\("(ASTRIA_[A-Z0-9_]+)"\)/g)) envRead.add(m[1]);
for (const m of cliSources.matchAll(/process\.env\.(ASTRIA_[A-Z0-9_]+)/g)) envRead.add(m[1]);
// CLI-side helper with computed access (hook-guard.ts: envVar('HOOK_STRICT'))
for (const m of cliSources.matchAll(/envVar\('([A-Z0-9_]+)'\)/g)) envRead.add(`ASTRIA_${m[1]}`);
const envDoc = read('website/docs/reference/env-vars.md');
const docEnv = new Set([...envDoc.matchAll(/`(ASTRIA_[A-Z0-9_]+)`/g)].map((m) => m[1]));
for (const v of [...envRead].sort()) {
  if (!docEnv.has(v)) problems.push(`env-vars.md: \`${v}\` is read by the code but not documented`);
}
for (const v of [...docEnv].sort()) {
  if (!envRead.has(v)) problems.push(`env-vars.md: \`${v}\` is documented but never read`);
}
for (const m of envDoc.matchAll(/`((?:OPENAI|GEMINI|GOOGLE|NEO4J|TYPESAFE)_[A-Z0-9_]+)`/g)) {
  if (!prodRust.includes(m[1]) && !cliSources.includes(m[1])) {
    problems.push(`env-vars.md: \`${m[1]}\` is documented but never appears in code`);
  }
}

// 8. CLI surface: every registered command word and long flag appears in the
//    CLI reference. This is the check that would have caught --label-communities
//    and --deep shipping without reference-page coverage.
const cliSrc = read('packages/astria-cli/src/index.ts');
const cliDoc = read('website/docs/reference/cli.md');
for (const m of cliSrc.matchAll(/\.command\('([\w-]+)/g)) {
  if (!cliDoc.includes(m[1])) {
    problems.push(`cli.md: command \`${m[1]}\` is registered but not documented`);
  }
}
for (const m of cliSrc.matchAll(/\.option\('(--[a-z][a-z-]*)/g)) {
  if (!cliDoc.includes(m[1])) {
    problems.push(`cli.md: flag \`${m[1]}\` is registered but not documented`);
  }
}

// 9. MCP tools: every registered tool name appears in the MCP reference.
const mcpDoc = read('website/docs/reference/mcp-tools.md');
for (const m of read('crates/astria-mcp/src/lib.rs').matchAll(/"name": "([a-z_]+)"/g)) {
  if (!mcpDoc.includes(m[1])) {
    problems.push(`mcp-tools.md: tool \`${m[1]}\` is registered but not documented`);
  }
}

// Cache/evidence contracts are user-facing guarantees in both architecture pages.
const storeSource = read('crates/astria-query/src/store.rs');
const capacity = storeSource.match(/fn clamp_capacity[^]*?const FLOOR: usize = (\d+);[^]*?const CEILING: usize = (\d+);[^]*?const DEFAULT: usize = (\d+);/);
for (const file of ['ARCHITECTURE.md', 'website/docs/explanation/architecture.md']) {
  const text = read(file);
  if (!text.includes('generation-keyed bounded snapshot cache per process') || !text.includes('Unstamped databases bypass the cache')) problems.push(file + ': snapshot-cache contract missing');
  if (capacity && (!text.includes('defaults to ' + capacity[3] + ' entries') || !text.includes('ASTRIA_SNAPSHOT_CACHE_ENTRIES\` ' + capacity[1] + '..=' + capacity[2]))) problems.push(file + ': snapshot-cache capacity differs from code');
  if (!text.includes('Name-derived call and import bindings carry \`RESOLVED\`') || !text.includes('only \`EXTRACTED\`/\`DECLARED\` evidence')) problems.push(file + ': resolved-binding/evidence-tier contract missing');
  if (/no process-global graph cache|Resolved targets are \`EXTRACTED\`|Name-based resolution is still \`INFERRED\`/.test(text)) problems.push(file + ': obsolete cache or evidence claim');
}
if (!capacity) problems.push('store.rs: snapshot cache capacity declarations not found');
if (!read('crates/astria-extract/src/refs.rs').includes('"RESOLVED"')) problems.push('refs.rs: docs claim RESOLVED bindings but source does not');
if (!read('crates/astria-query/src/lib.rs').includes('"EXTRACTED" | "DECLARED"')) problems.push('query: high-detail evidence contract changed');

if (problems.length) {
  console.error('docs drift detected:');
  for (const p of problems) console.error(`  - ${p}`);
  process.exit(1);
}
console.log(
  `docs in sync: ${members.length} crates, ${relations.size} emitted + ${documentedRelations.size} documented relations, ` +
    `${envRead.size} env vars, rust-version ${rustVersion}, node ${badgeMajor}`,
);
