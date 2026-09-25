/**
 * Git hook installer tests — validates install, legacy-format migration,
 * append-to-existing, uninstall, and status. Uses temp git repos.
 *
 * Run with: npx tsx src/__tests__/hooks.test.ts
 */

import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { execSync } from 'child_process';

import { installGitHooks, uninstallGitHooks, statusGitHooks } from '../install/hooks';

let passed = 0;
let failed = 0;

function assert(condition: boolean, message: string) {
  if (condition) {
    passed++;
  } else {
    failed++;
    console.error(`FAIL: ${message}`);
  }
}

function tmpGitRepo(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'graphify-hooks-test-'));
  execSync('git init', { cwd: dir, stdio: 'pipe' });
  return dir;
}

function hookPath(repo: string, name: string): string {
  return path.join(repo, '.git', 'hooks', name);
}

// Legacy shell-format hook written by releases before 0.3.0.
const LEGACY_CHECKOUT = `#!/bin/sh

# nodesify-graphify-checkout-hook-start
[ "$3" != "1" ] && exit 0
[ ! -d ".graphify" ] && exit 0
nodesify-graphify update . || true
# nodesify-graphify-checkout-hook-end
`;

function testFreshInstall() {
  const repo = tmpGitRepo();
  const results = installGitHooks(repo);
  assert(results.length === 2, 'fresh install: two hooks reported');
  const content = fs.readFileSync(hookPath(repo, 'post-commit'), 'utf-8');
  assert(content.startsWith('#!/usr/bin/env node'), 'fresh install: node shebang');
  assert(content.includes('// nodesify-graphify-hook-start'), 'fresh install: current marker present');
  assert(!content.includes('# nodesify-graphify-hook-start'), 'fresh install: no legacy marker');
}

function testLegacyMigration() {
  const repo = tmpGitRepo();
  fs.writeFileSync(hookPath(repo, 'post-checkout'), LEGACY_CHECKOUT, 'utf-8');

  const results = installGitHooks(repo);
  const content = fs.readFileSync(hookPath(repo, 'post-checkout'), 'utf-8');
  assert(content.includes('// nodesify-graphify-checkout-hook-start'), 'migration: current marker installed');
  assert(!content.includes('# nodesify-graphify-checkout-hook-start'), 'migration: legacy marker removed');
  assert(content.trim().startsWith('#!'), 'migration: file still starts with a shebang');
  assert(results.some(r => r.includes('migrated') || r.includes('replaced legacy')), 'migration: reported');
}

function testLegacyAlongsideCurrentNotDuplicated() {
  const repo = tmpGitRepo();
  // Legacy section + already-appended current section (the broken state the
  // old installer produced on legacy machines).
  const broken = LEGACY_CHECKOUT + '\n// nodesify-graphify-checkout-hook-start\nconst x = 1;\n// nodesify-graphify-checkout-hook-end\n';
  fs.writeFileSync(hookPath(repo, 'post-checkout'), broken, 'utf-8');

  installGitHooks(repo);
  const content = fs.readFileSync(hookPath(repo, 'post-checkout'), 'utf-8');
  const currentCount = content.split('// nodesify-graphify-checkout-hook-start').length - 1;
  assert(currentCount === 1, 'dedupe: exactly one current section');
  assert(!content.includes('# nodesify-graphify-checkout-hook-start'), 'dedupe: legacy section gone');
}

function testAppendToForeignHook() {
  const repo = tmpGitRepo();
  fs.writeFileSync(hookPath(repo, 'post-commit'), '#!/bin/sh\necho own hook\n', 'utf-8');

  const results = installGitHooks(repo);
  const content = fs.readFileSync(hookPath(repo, 'post-commit'), 'utf-8');
  assert(content.includes('echo own hook'), 'append: foreign content preserved');
  assert(content.includes('// nodesify-graphify-hook-start'), 'append: graphify section added');
  assert(results.some(r => r.includes('appended')), 'append: reported as appended');
}

function testIdempotentInstall() {
  const repo = tmpGitRepo();
  installGitHooks(repo);
  const results = installGitHooks(repo);
  assert(results.every(r => r.includes('already installed')), 'idempotent: second install is a no-op');
}

function testUninstallRemovesBothFormats() {
  const repo = tmpGitRepo();
  const broken = LEGACY_CHECKOUT + '\n// nodesify-graphify-checkout-hook-start\nconst x = 1;\n// nodesify-graphify-checkout-hook-end\n';
  fs.writeFileSync(hookPath(repo, 'post-checkout'), broken, 'utf-8');

  const results = uninstallGitHooks(repo);
  const p = hookPath(repo, 'post-checkout');
  // The file held only our sections + shebang, so uninstall deletes it entirely
  if (fs.existsSync(p)) {
    assert(!fs.readFileSync(p, 'utf-8').includes('nodesify-graphify'), 'uninstall: both formats removed');
  } else {
    assert(true, 'uninstall: empty hook file deleted');
  }
  assert(results.some(r => r.includes('removed')), 'uninstall: reported removed');
}

function testStatusDetectsLegacy() {
  const repo = tmpGitRepo();
  fs.writeFileSync(hookPath(repo, 'post-checkout'), LEGACY_CHECKOUT, 'utf-8');

  const results = statusGitHooks(repo);
  assert(results.some(r => r.includes('post-checkout: installed (legacy format')), 'status: legacy detected with migration hint');
  assert(results.some(r => r.includes('post-commit: not installed')), 'status: missing hook reported');
}

function testNotAGitRepo() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'graphify-nogit-'));
  const results = installGitHooks(dir);
  assert(results.length === 1 && results[0] === 'Not a git repository', 'no-repo: graceful message');
}

function main() {
  testFreshInstall();
  testLegacyMigration();
  testLegacyAlongsideCurrentNotDuplicated();
  testAppendToForeignHook();
  testIdempotentInstall();
  testUninstallRemovesBothFormats();
  testStatusDetectsLegacy();
  testNotAGitRepo();

  console.log(`\n${passed} passed, ${failed} failed (git hooks)`);
  if (failed > 0) process.exit(1);
}

main();
