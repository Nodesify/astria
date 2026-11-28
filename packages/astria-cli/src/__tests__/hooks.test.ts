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

import { installGitHooks, uninstallGitHooks, statusGitHooks, resolveCliEntry } from '../install/hooks';

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

// The CLI a hook script records.
function recordedEntry(hook: string): string | null {
  const match = hook.match(/^const ASTRIA_CLI_ENTRY = (".*");$/m);
  return match ? (JSON.parse(match[1]) as string) : null;
}

// Point an installed hook at another CLI entry, the state a hook is in
// after its install moved or was removed. False when the hook records none.
function pointHookAt(file: string, entry: string): boolean {
  const before = fs.readFileSync(file, 'utf-8');
  const after = before.replace(
    /^const ASTRIA_CLI_ENTRY = ".*";$/m,
    `const ASTRIA_CLI_ENTRY = ${JSON.stringify(entry)};`,
  );
  fs.writeFileSync(file, after, 'utf-8');
  return after !== before;
}

// A stand-in for the CLI a hook records: it writes the arguments it was
// given, and its working directory, to a log beside itself.
const FAKE_CLI = "const fs = require('fs');\nconst path = require('path');\nfs.appendFileSync(path.join(__dirname, 'fake-cli.log'), JSON.stringify({ args: process.argv.slice(2), cwd: process.cwd() }) + '\\n');\n";

// What the hook used to run first: a file at that path in whichever
// repository the commit happens in.
const PLANTED_CLI = "require('fs').writeFileSync('planted-ran.txt', 'ran');\n";

function commitNotes(repo: string, contents: string): void {
  fs.writeFileSync(path.join(repo, 'notes.md'), contents, 'utf-8');
  execSync('git add notes.md', { cwd: repo, stdio: 'pipe' });
  execSync('git -c user.name=t -c user.email=t@example.com commit -m notes', { cwd: repo, stdio: 'pipe' });
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
  assert(results.length === 3, 'fresh install: three hooks reported');
  const content = fs.readFileSync(hookPath(repo, 'post-commit'), 'utf-8');
  assert(content.startsWith('#!/usr/bin/env node'), 'fresh install: node shebang');
  assert(content.includes('// astria-hook-start'), 'fresh install: current marker present');
  assert(content.includes('ASTRIA_HOOK_VERSION'), 'fresh install: version sentinel present');
  assert(content.includes("'update', '.', '--quiet'"), 'fresh install: hook inspects inputs quietly');
  assert(!content.includes('--if-stale'), 'fresh install: recent graphs still inspect changed inputs');
  assert(!content.includes('nodesify-graphify-hook'), 'fresh install: no legacy marker');
  // The hook runs one CLI, the one that installed it, by absolute path.
  const entry = recordedEntry(content);
  assert(entry !== null && path.isAbsolute(entry), 'fresh install: records an absolute CLI entry');
  assert(!content.includes("'node packages/astria-cli"), 'fresh install: no path relative to the repository');
  assert(!content.includes('npx --no-install'), 'fresh install: nothing left for npx to resolve');
  // Every shell call in the hook names a literal git command: no variable
  // ever reaches a shell.
  assert(!/execSync\(\s*[A-Za-z_]/.test(content), 'fresh install: no variable command reaches a shell');
  assert(content.includes('execFileSync(process.execPath'), 'fresh install: the entry runs with this Node');
}

// The hook used to try `node packages/astria-cli/dist/index.js` first, a
// path relative to whichever repository the commit happens in: any
// repository that ships a file there had it run on every commit, checkout
// and merge. Now the only CLI it runs is the one recorded at install time.
function testHookRunsOnlyTheCliThatInstalledIt() {
  const repo = tmpGitRepo();
  commitNotes(repo, 'one\n');

  const cli = path.join(repo, 'fake-cli.js');
  const log = path.join(repo, 'fake-cli.log');
  fs.writeFileSync(cli, FAKE_CLI, 'utf-8');
  const planted = path.join(repo, 'packages', 'astria-cli', 'dist');
  fs.mkdirSync(planted, { recursive: true });
  fs.writeFileSync(path.join(planted, 'index.js'), PLANTED_CLI, 'utf-8');
  fs.mkdirSync(path.join(repo, '.astria'));

  installGitHooks(repo, cli);
  commitNotes(repo, 'one\ntwo\n');

  assert(!fs.existsSync(path.join(repo, 'planted-ran.txt')), 'dispatch: a CLI shipped by the repository does not run');
  assert(fs.existsSync(log), 'dispatch: the CLI that installed the hook ran');
  if (fs.existsSync(log)) {
    const run = JSON.parse(fs.readFileSync(log, 'utf-8').trim());
    assert(run.args.join(' ') === 'update . --quiet', 'dispatch: with the update arguments');
    assert(path.resolve(run.cwd) === path.resolve(repo), 'dispatch: from the repository root');
  }
}

// A hook outlives the install that wrote it. When that install moves (a
// package manager that keeps each version in its own directory) or the
// user runs `hook install` from a second one, the status says so and the
// installer re-links the hook.
function testInstallRelinksAHookWhoseCliMoved() {
  const repo = tmpGitRepo();
  const cli = path.join(repo, 'here', 'index.js');
  fs.mkdirSync(path.dirname(cli));
  fs.writeFileSync(cli, '', 'utf-8');
  installGitHooks(repo, cli);
  const file = hookPath(repo, 'post-commit');
  const fresh = fs.readFileSync(file, 'utf-8');
  assert(statusGitHooks(repo).every(r => r.endsWith(': installed')), 'moved: status is clean while the CLI is where it was recorded');

  assert(pointHookAt(file, path.join(repo, 'moved-away', 'index.js')), 'moved: hook pointed at a CLI that is gone');
  assert(statusGitHooks(repo).some(r => r.includes('post-commit: installed, but its CLI is gone')), 'moved: status reports the missing CLI');

  const results = installGitHooks(repo, cli);
  assert(results.includes('post-commit: re-linked to this CLI'), 'moved: install re-links the hook');
  assert(fs.readFileSync(file, 'utf-8') === fresh, 'moved: the hook is what a fresh install writes');
  assert(statusGitHooks(repo).every(r => r.endsWith(': installed')), 'moved: status is clean again');
}

// The path the CLI was invoked by is recorded when it is the CLI under
// another name: a package manager's symlink outlives the version it points
// at, where the real path does not.
function testResolveCliEntry() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-entry-'));
  const real = path.join(dir, 'real', 'index.js');
  fs.mkdirSync(path.dirname(real));
  fs.writeFileSync(real, '', 'utf-8');
  const same = (a: string, b: string): boolean => {
    try {
      return fs.realpathSync(a) === fs.realpathSync(b);
    } catch {
      return false;
    }
  };
  assert(resolveCliEntry(undefined, real, same) === real, 'entry: no invocation path gives the entry beside');
  assert(resolveCliEntry('relative/index.js', real, same) === real, 'entry: a relative path is never recorded');
  const other = path.join(dir, 'other.js');
  fs.writeFileSync(other, '', 'utf-8');
  assert(resolveCliEntry(other, real, same) === real, 'entry: another file is not the CLI');

  const invoked = path.join(dir, 'astria');
  try {
    fs.symlinkSync(real, invoked);
  } catch {
    // No privilege to create links on this machine.
    return;
  }
  assert(resolveCliEntry(invoked, real, same) === invoked, 'entry: the invoked symlink is kept');
  assert(resolveCliEntry(real, real, same) === real, 'entry: a direct invocation is kept');
}

function testPostMergeInstall() {
  const repo = tmpGitRepo();
  const results = installGitHooks(repo);
  assert(results.some(r => r.startsWith('post-merge: installed')), 'post-merge: installed');
  const content = fs.readFileSync(hookPath(repo, 'post-merge'), 'utf-8');
  assert(content.startsWith('#!/usr/bin/env node'), 'post-merge: node shebang');
  assert(content.includes('// astria-merge-hook-start'), 'post-merge: marker present');
  // The pre-merge tip covers every incoming commit in a fast-forward merge.
  assert(content.includes('ORIG_HEAD HEAD'), 'post-merge: diff from pre-merge tip');
  assert(content.includes('.astria'), 'post-merge: watches the graph directory');

  const status = statusGitHooks(repo);
  assert(status.some(r => r.includes('post-merge: installed')), 'post-merge: status reports installed');

  const removed = uninstallGitHooks(repo);
  assert(removed.some(r => r.includes('post-merge: removed')), 'post-merge: uninstall removes it');
  assert(!fs.existsSync(hookPath(repo, 'post-merge')), 'post-merge: hook file deleted when empty');
}

function testPostMergeAppendedToUserHook() {
  const repo = tmpGitRepo();
  // A user's own post-merge hook must survive; only a Node-script hook can
  // absorb the appended JavaScript updater (shell hooks are skipped).
  fs.writeFileSync(hookPath(repo, 'post-merge'), '#!/bin/sh\necho user merge hook\n', 'utf-8');
  const results = installGitHooks(repo);
  const content = fs.readFileSync(hookPath(repo, 'post-merge'), 'utf-8');
  assert(content.includes('echo user merge hook'), 'post-merge append: user hook preserved');
  assert(!content.includes('// astria-merge-hook-start'), 'post-merge append: shell hook left alone');
  assert(results.some(r => r.startsWith('post-merge: skipped')), 'post-merge append: skip reported');
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
  // A shell hook: appending the JavaScript updater would break it (git
  // executes hook files with sh when they lack a node shebang — the old
  // append produced "syntax error near unexpected token" on every commit),
  // so install must skip with a notice instead.
  fs.writeFileSync(hookPath(repo, 'post-commit'), '#!/bin/sh\necho own hook\n', 'utf-8');

  const results = installGitHooks(repo);
  const content = fs.readFileSync(hookPath(repo, 'post-commit'), 'utf-8');
  assert(content.includes('echo own hook'), 'append: foreign content preserved');
  assert(!content.includes('// astria-hook-start'), 'append: shell hook left alone');
  assert(results.some(r => r.includes('skipped')), 'append: skip reported');
}

function testAppendToForeignNodeHook() {
  const repo = tmpGitRepo();
  // A Node-script hook absorbs the appended updater — the one foreign shape
  // where appending is safe.
  fs.writeFileSync(hookPath(repo, 'post-commit'), '#!/usr/bin/env node\nconsole.log("own");\n', 'utf-8');

  const results = installGitHooks(repo);
  const content = fs.readFileSync(hookPath(repo, 'post-commit'), 'utf-8');
  assert(content.includes('console.log("own");'), 'node append: foreign content preserved');
  assert(content.includes('// astria-hook-start'), 'node append: astria section added');
  assert(results.some(r => r.includes('appended')), 'node append: reported as appended');
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
  assert(results.some(r => r.includes('post-merge: not installed')), 'status: missing post-merge reported');
}

function testNotAGitRepo() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'astria-nogit-'));
  const results = installGitHooks(dir);
  assert(results.length === 1 && results[0] === 'Not a git repository', 'no-repo: graceful message');
}

function main() {
  testFreshInstall();
  testHookRunsOnlyTheCliThatInstalledIt();
  testInstallRelinksAHookWhoseCliMoved();
  testResolveCliEntry();
  testPostMergeInstall();
  testPostMergeAppendedToUserHook();
  testShellLegacyMigration();
  testJsLegacyMigration();
  testLegacyAlongsideCurrentNotDuplicated();
  testAppendToForeignHook();
  testAppendToForeignNodeHook();
  testIdempotentInstall();
  testUninstallRemovesAllFormats();
  testStatusDetectsLegacy();
  testNotAGitRepo();

  console.log(`\n${passed} passed, ${failed} failed (git hooks)`);
  if (failed > 0) process.exit(1);
}

main();
