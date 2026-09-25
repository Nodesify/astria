/**
 * Git hook installer tests — validates install, legacy-format migration
 * (both the pre-1.0 nodesify-graphify JS hooks and the pre-0.3 shell hooks),
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
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-hooks-test-'));
  execSync('git init', { cwd: dir, stdio: 'pipe' });
  return dir;
}

function hookPath(repo: string, name: string): string {
  return path.join(repo, '.git', 'hooks', name);
}

// Legacy shell-format hook written by releases before 0.3.0.
const SHELL_LEGACY_CHECKOUT = `#!/bin/sh

# nodesify-graphify-checkout-hook-start
[ "$3" != "1" ] && exit 0
[ ! -d ".graphify" ] && exit 0
nodesify-graphify update . || true
# nodesify-graphify-checkout-hook-end
`;

// Legacy JS-format hook written by 0.3–0.9 (nodesify-graphify era).
const JS_LEGACY_CHECKOUT = `#!/usr/bin/env node

// nodesify-graphify-checkout-hook-start
const { execSync } = require('child_process');
const { existsSync } = require('fs');
if (process.argv[3] !== '1') process.exit(0);
if (!existsSync('.graphify')) process.exit(0);
execSync('nodesify-graphify update .', { stdio: 'inherit' });
// nodesify-graphify-checkout-hook-end
`;

function testFreshInstall() {
  const repo = tmpGitRepo();
  const results = installGitHooks(repo);
  assert(results.length === 2, 'fresh install: two hooks reported');
  const content = fs.readFileSync(hookPath(repo, 'post-commit'), 'utf-8');
  assert(content.startsWith('#!/usr/bin/env node'), 'fresh install: node shebang');
  assert(content.includes('// astria-hook-start'), 'fresh install: current marker present');
  assert(content.includes('ASTRIA_HOOK_VERSION'), 'fresh install: version sentinel present');
  assert(!content.includes('nodesify-graphify'), 'fresh install: no legacy marker');
}

function testShellLegacyMigration() {
  const repo = tmpGitRepo();
  fs.writeFileSync(hookPath(repo, 'post-checkout'), SHELL_LEGACY_CHECKOUT, 'utf-8');

  const results = installGitHooks(repo);
  const content = fs.readFileSync(hookPath(repo, 'post-checkout'), 'utf-8');
  assert(content.includes('// astria-checkout-hook-start'), 'shell migration: current marker installed');
  assert(!content.includes('# nodesify-graphify-checkout-hook-start'), 'shell migration: legacy marker removed');
  assert(content.trim().startsWith('#!'), 'shell migration: file still starts with a shebang');
  assert(results.some(r => r.includes('migrated') || r.includes('replaced legacy')), 'shell migration: reported');
}

function testJsLegacyMigration() {
  const repo = tmpGitRepo();
  fs.writeFileSync(hookPath(repo, 'post-checkout'), JS_LEGACY_CHECKOUT, 'utf-8');

  installGitHooks(repo);
  const content = fs.readFileSync(hookPath(repo, 'post-checkout'), 'utf-8');
  assert(content.includes('// astria-checkout-hook-start'), 'js migration: current marker installed');
  assert(!content.includes('nodesify-graphify-checkout-hook-start'), 'js migration: 0.9-era marker removed');
  assert(!content.includes('.graphify'), 'js migration: old data-folder reference gone');
  assert(content.includes('.astria'), 'js migration: new template watches .astria');
}

function testLegacyAlongsideCurrentNotDuplicated() {
  const repo = tmpGitRepo();
  // Legacy section + already-appended current section (the broken state the
  // old installer produced on legacy machines).
  const broken = SHELL_LEGACY_CHECKOUT + '\n// nodesify-graphify-checkout-hook-start\nconst x = 1;\n// nodesify-graphify-checkout-hook-end\n';
  fs.writeFileSync(hookPath(repo, 'post-checkout'), broken, 'utf-8');

  installGitHooks(repo);
  const content = fs.readFileSync(hookPath(repo, 'post-checkout'), 'utf-8');
  const currentCount = content.split('// astria-checkout-hook-start').length - 1;
  assert(currentCount === 1, 'dedupe: exactly one current section');
  assert(!content.includes('nodesify-graphify-checkout-hook-start'), 'dedupe: legacy sections gone');
}

function testAppendToForeignHook() {
  const repo = tmpGitRepo();
  fs.writeFileSync(hookPath(repo, 'post-commit'), '#!/bin/sh\necho own hook\n', 'utf-8');

  const results = installGitHooks(repo);
  const content = fs.readFileSync(hookPath(repo, 'post-commit'), 'utf-8');
  assert(content.includes('echo own hook'), 'append: foreign content preserved');
  assert(content.includes('// astria-hook-start'), 'append: astria section added');
  assert(results.some(r => r.includes('appended')), 'append: reported as appended');
}

function testIdempotentInstall() {
  const repo = tmpGitRepo();
  installGitHooks(repo);
  const results = installGitHooks(repo);
  assert(results.every(r => r.includes('already installed')), 'idempotent: second install is a no-op');
}

function testUninstallRemovesAllFormats() {
  const repo = tmpGitRepo();
  const broken = SHELL_LEGACY_CHECKOUT + '\n// nodesify-graphify-checkout-hook-start\nconst x = 1;\n// nodesify-graphify-checkout-hook-end\n';
  fs.writeFileSync(hookPath(repo, 'post-checkout'), broken, 'utf-8');

  const results = uninstallGitHooks(repo);
  const p = hookPath(repo, 'post-checkout');
  // The file held only our sections + shebang, so uninstall deletes it entirely
  if (fs.existsSync(p)) {
    assert(!fs.readFileSync(p, 'utf-8').includes('nodesify-graphify'), 'uninstall: all formats removed');
  } else {
    assert(true, 'uninstall: empty hook file deleted');
  }
  assert(results.some(r => r.includes('removed')), 'uninstall: reported removed');

  // The 0.9-era JS hook alone is fully removed too.
  const repo2 = tmpGitRepo();
  fs.writeFileSync(hookPath(repo2, 'post-checkout'), JS_LEGACY_CHECKOUT, 'utf-8');
  uninstallGitHooks(repo2);
  const p2 = hookPath(repo2, 'post-checkout');
  if (fs.existsSync(p2)) {
    assert(!fs.readFileSync(p2, 'utf-8').includes('nodesify-graphify'), 'uninstall: js-era hook removed');
  } else {
    assert(true, 'uninstall: js-era hook file deleted');
  }
}

function testStatusDetectsLegacy() {
  const repo = tmpGitRepo();
  fs.writeFileSync(hookPath(repo, 'post-checkout'), SHELL_LEGACY_CHECKOUT, 'utf-8');

  const results = statusGitHooks(repo);
  assert(results.some(r => r.includes('post-checkout: installed (legacy format')), 'status: legacy detected with migration hint');
  assert(results.some(r => r.includes('post-commit: not installed')), 'status: missing hook reported');
}

function testNotAGitRepo() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-nogit-'));
  const results = installGitHooks(dir);
  assert(results.length === 1 && results[0] === 'Not a git repository', 'no-repo: graceful message');
}

function main() {
  testFreshInstall();
  testShellLegacyMigration();
  testJsLegacyMigration();
  testLegacyAlongsideCurrentNotDuplicated();
  testAppendToForeignHook();
  testIdempotentInstall();
  testUninstallRemovesAllFormats();
  testStatusDetectsLegacy();
  testNotAGitRepo();

  console.log(`\n${passed} passed, ${failed} failed (git hooks)`);
  if (failed > 0) process.exit(1);
}

main();
