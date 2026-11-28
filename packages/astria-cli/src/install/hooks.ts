import * as fs from 'fs';
import * as path from 'path';
import { execSync } from 'child_process';
import { writeTextAtomic, withInstallLock } from './atomic';

/// Bumped whenever the hook scripts change, so that the installer refreshes
/// the hooks an earlier release wrote.
const HOOK_VERSION = '6';

/// Whether two paths name the same file.
function sameFile(a: string, b: string): boolean {
  try {
    return fs.realpathSync(a) === fs.realpathSync(b);
  } catch {
    return false;
  }
}

/// The CLI entry a hook records and runs. The path this process was invoked
/// by is preferred when it is the entry next to this file under another
/// name: a package manager's symlink (`/opt/homebrew/bin/astria`) outlives
/// the versioned directory it points at, where the real path does not. Any
/// other invocation — a test runner, a tool that `require`s this module —
/// gets the entry next to this file.
function cliEntry(): string {
  return resolveCliEntry(process.argv[1], path.resolve(__dirname, '..', 'index.js'), sameFile);
}

/// `cliEntry` with its inputs passed in, so the choice is testable without
/// process state.
export function resolveCliEntry(
  invoked: string | undefined,
  beside: string,
  same: (a: string, b: string) => boolean,
): string {
  if (invoked && path.isAbsolute(invoked) && same(invoked, beside)) return invoked;
  return beside;
}

const VERSION_SENTINEL = `ASTRIA_HOOK_VERSION = '${HOOK_VERSION}'`;

/// The line of a hook script that records the CLI it runs.
function cliEntryLine(entry: string): string {
  return `const ASTRIA_CLI_ENTRY = ${JSON.stringify(entry)};`;
}

/// The CLI a hook script records, or null when it records none (a script
/// written before 1.7.1).
function recordedEntry(hookContent: string): string | null {
  const match = hookContent.match(/^const ASTRIA_CLI_ENTRY = (".*");$/m);
  return match ? (JSON.parse(match[1]) as string) : null;
}

function updateHelper(entry: string): string {
  return `
const ASTRIA_HOOK_VERSION = '${HOOK_VERSION}';
// The hook runs the CLI that installed it: the absolute path of its entry,
// recorded at install time, started with the Node that runs this hook.
// Nothing is looked up when a commit happens. A path relative to the
// repository, or a name left to npx or the shell to resolve, lets the
// repository being committed — or whatever a script has put first on PATH —
// choose what runs here.
${cliEntryLine(entry)}
const { execFileSync } = require('child_process');
function runAstriaUpdate() {
  if (!existsSync(ASTRIA_CLI_ENTRY)) {
    console.error('[astria] the CLI that installed this hook is gone (' + ASTRIA_CLI_ENTRY + ') - run "astria hook install" to re-link it');
    return;
  }
  // --quiet keeps the refresh out of the Git operation's output. A failure
  // is swallowed: a hook must never break a commit.
  try {
    execFileSync(process.execPath, [ASTRIA_CLI_ENTRY, 'update', '.', '--quiet'], { stdio: 'inherit' });
  } catch {}
}
`;
}

const postCommitScript = (entry: string): string => `// astria-hook-start
const { execSync } = require('child_process');
const { existsSync } = require('fs');
const path = require('path');
${updateHelper(entry)}
try {
  // No shell redirects here: execSync uses cmd.exe on Windows where
  // "2>/dev/null" fails with "The system cannot find the path specified".
  // execSync captures stderr into the error object anyway.
  const gitDir = execSync('git rev-parse --git-dir', { encoding: 'utf-8' }).trim();
  const checks = [
    path.join(gitDir, 'rebase-merge'),
    path.join(gitDir, 'rebase-apply'),
    path.join(gitDir, 'MERGE_HEAD'),
    path.join(gitDir, 'CHERRY_PICK_HEAD'),
  ];
  if (checks.some(p => existsSync(p))) process.exit(0);

  const changed = execSync('git diff --name-only HEAD~1 HEAD || git diff --name-only HEAD', { encoding: 'utf-8' }).trim();
  if (!changed) process.exit(0);

  if (existsSync('.astria')) runAstriaUpdate();
} catch {}
// astria-hook-end
`;

const postCheckoutScript = (entry: string): string => `// astria-checkout-hook-start
const { execSync } = require('child_process');
const { existsSync } = require('fs');
const path = require('path');
${updateHelper(entry)}
const branchSwitch = process.argv[3];
if (branchSwitch !== '1') process.exit(0);
if (!existsSync('.astria')) process.exit(0);

try {
  // No shell redirects — see the note in the post-commit script.
  const gitDir = execSync('git rev-parse --git-dir', { encoding: 'utf-8' }).trim();
  if (existsSync(path.join(gitDir, 'rebase-merge')) || existsSync(path.join(gitDir, 'rebase-apply'))) process.exit(0);

  console.log('[astria] Branch switched - rebuilding knowledge graph...');
  runAstriaUpdate();
} catch {}
// astria-checkout-hook-end
`;

// post-merge runs after true merges and fast-forward merges, including pulls.
// ORIG_HEAD records the branch tip before the merge, covering every incoming
// commit in a multi-commit fast-forward. Squash merges refresh on commit.
const postMergeScript = (entry: string): string => `// astria-merge-hook-start
const { execSync } = require('child_process');
const { existsSync } = require('fs');
const path = require('path');
${updateHelper(entry)}
if (!existsSync('.astria')) process.exit(0);

try {
  // No shell redirects — see the note in the post-commit script.
  const gitDir = execSync('git rev-parse --git-dir', { encoding: 'utf-8' }).trim();
  if (existsSync(path.join(gitDir, 'rebase-merge')) || existsSync(path.join(gitDir, 'rebase-apply'))) process.exit(0);

  // Compare the pre-merge tip with the final tip for both merge kinds.
  const changed = execSync('git diff --name-only ORIG_HEAD HEAD', { encoding: 'utf-8' }).trim();
  if (!changed) process.exit(0);

  // Let update inspect graph inputs; extension allowlists miss documentation,
  // manifests, configuration, and newly supported languages.
  console.log('[astria] Merge completed - refreshing knowledge graph...');
  runAstriaUpdate();
} catch {}
// astria-merge-hook-end
`;

interface HookDef {
  hookName: string;
  /// The hook script, for the CLI entry it is to record and run.
  script: (cliEntry: string) => string;
  startMarker: string;
  endMarker: string;
  // Older releases wrote sections under other markers: 0.3–0.9 shipped the
  // nodesify-graphify-named JS hooks, and pre-0.3 wrote shell-format hooks
  // with '#' markers. The current installer must recognize and replace both,
  // otherwise it appends a second section that runs the graph twice.
  legacyMarkers: Array<{ startMarker: string; endMarker: string }>;
}

const LEGACY_HOOK_PREFIXES = [
  { js: 'nodesify-graphify' }, // 0.3–0.9 JS-format hooks
  { shell: 'nodesify-graphify' }, // pre-0.3 shell-format hooks
];

const HOOK_DEFS: HookDef[] = [
  {
    hookName: 'post-commit',
    script: postCommitScript,
    startMarker: '// astria-hook-start',
    endMarker: '// astria-hook-end',
    legacyMarkers: [
      { startMarker: '// nodesify-graphify-hook-start', endMarker: '// nodesify-graphify-hook-end' },
      { startMarker: '# nodesify-graphify-hook-start', endMarker: '# nodesify-graphify-hook-end' },
    ],
  },
  {
    hookName: 'post-checkout',
    script: postCheckoutScript,
    startMarker: '// astria-checkout-hook-start',
    endMarker: '// astria-checkout-hook-end',
    legacyMarkers: [
      { startMarker: '// nodesify-graphify-checkout-hook-start', endMarker: '// nodesify-graphify-checkout-hook-end' },
      { startMarker: '# nodesify-graphify-checkout-hook-start', endMarker: '# nodesify-graphify-checkout-hook-end' },
    ],
  },
  {
    hookName: 'post-merge',
    script: postMergeScript,
    startMarker: '// astria-merge-hook-start',
    endMarker: '// astria-merge-hook-end',
    // No legacy release ever shipped a post-merge hook, so there is no
    // earlier marker family to recognize or replace here.
    legacyMarkers: [],
  },
];

const SHEBANGS = ['#!/bin/sh', '#!/bin/bash', '#!/usr/bin/env node'];

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function stripMarkerSection(content: string, startMarker: string, endMarker: string): string {
  const regex = new RegExp(
    '\\n*' + escapeRegExp(startMarker) + '[\\s\\S]*?' + escapeRegExp(endMarker) + '\\n*',
    'g'
  );
  return content.replace(regex, '\n');
}

function stripAllLegacySections(content: string, def: HookDef): string {
  for (const legacy of def.legacyMarkers) {
    content = stripMarkerSection(content, legacy.startMarker, legacy.endMarker);
  }
  return content;
}

function hasLegacyMarker(content: string, def: HookDef): boolean {
  return def.legacyMarkers.some((l) => content.includes(l.startMarker));
}

function isOwnShebangOnly(content: string): boolean {
  const trimmed = content.trim();
  return trimmed === '' || SHEBANGS.includes(trimmed);
}

/// The appended updater is JavaScript — appending it to a hook git runs with
/// sh/bash (a user's shell hook, a husky-style hook) breaks that hook with
/// syntax errors on every commit. Only a Node-script hook can absorb it.
function isNodeScriptHook(content: string): boolean {
  const firstLine = content.split('\n', 1)[0].trim();
  return firstLine.startsWith('#!') && firstLine.includes('node');
}

function getGitRoot(projectDir: string): string | null {
  try {
    const result = execSync('git rev-parse --show-toplevel', {
      cwd: projectDir,
      encoding: 'utf-8',
    }).trim();
    return result;
  } catch {
    return null;
  }
}

function getHooksDir(gitRoot: string): string {
  try {
    const customPath = execSync('git config core.hooksPath', {
      cwd: gitRoot,
      encoding: 'utf-8',
    }).trim();
    if (customPath) {
      return path.isAbsolute(customPath) ? customPath : path.join(gitRoot, customPath);
    }
  } catch {
    // no custom hooks path
  }
  return path.join(gitRoot, '.git', 'hooks');
}

/// Git hook names come from the fixed HOOK_DEFS allowlist; guard the join
/// anyway so no runtime-influenced name can reach the hooks directory path.
function safeHookName(name: string): boolean {
  return /^[a-zA-Z0-9._-]+$/.test(name);
}

function hookPathOrNull(hooksDir: string, hookName: string): string | null {
  if (!safeHookName(hookName)) return null;
  return path.join(hooksDir, hookName);
}

function installHook(hooksDir: string, def: HookDef, entry: string): string {
  const hookPath = hookPathOrNull(hooksDir, def.hookName);
  if (!hookPath) return `${def.hookName}: skipped (unsafe hook name)`;
  if (!fs.existsSync(hooksDir)) {
    fs.mkdirSync(hooksDir, { recursive: true });
  }

  if (fs.existsSync(hookPath)) {
    let content = fs.readFileSync(hookPath, 'utf-8');
    const hadLegacy = hasLegacyMarker(content, def);
    if (hadLegacy) {
      content = stripAllLegacySections(content, def);
    }

    if (isOwnShebangOnly(content)) {
      // File contained only our legacy section - rewrite fresh in current format
      writeTextAtomic(hookPath, '#!/usr/bin/env node\n\n' + def.script(entry));
      return hadLegacy
        ? `${def.hookName}: migrated legacy hook to current format`
        : `${def.hookName}: installed`;
    }

    if (content.includes(def.startMarker)) {
      // Refresh the script body when it predates the current template
      // (sentinel: ASTRIA_HOOK_VERSION resolver). Without this, fixed
      // templates would never reach already-installed hooks.
      const current = content.includes(VERSION_SENTINEL);
      // And when it records another CLI: one that has moved away or been
      // removed (a versioned install directory), or a second install that
      // the user now runs `hook install` from.
      if (!current || !content.includes(cliEntryLine(entry))) {
        content = stripMarkerSection(content, def.startMarker, def.endMarker);
        const refreshed =
          content.trim() === '' || SHEBANGS.includes(content.trim())
            ? '#!/usr/bin/env node\n\n' + def.script(entry)
            : content.trimEnd() + '\n\n' + def.script(entry);
        writeTextAtomic(hookPath, refreshed);
        return current
          ? `${def.hookName}: re-linked to this CLI`
          : `${def.hookName}: updated script to current version`;
      }
      if (hadLegacy) {
        writeTextAtomic(hookPath, content);
        return `${def.hookName}: already installed (stale legacy section removed)`;
      }
      return `${def.hookName}: already installed`;
    }

    // Appending is only safe when the existing hook is a Node script; git
    // executes hook files with sh when they lack a node shebang, and a
    // JavaScript block appended to shell code breaks the hook (syntax
    // errors on every commit) instead of adding the updater.
    if (!isNodeScriptHook(content)) {
      return `${def.hookName}: skipped (existing hook is not a Node script; appending the astria updater would break it — bridge it manually if you want graph refreshes)`;
    }

    writeTextAtomic(hookPath, content.trimEnd() + '\n\n' + def.script(entry));
    return hadLegacy
      ? `${def.hookName}: appended to existing hook (replaced legacy section)`
      : `${def.hookName}: appended to existing hook`;
  }

  writeTextAtomic(hookPath, '#!/usr/bin/env node\n\n' + def.script(entry));
  try { fs.chmodSync(hookPath, 0o755); } catch { /* Windows */ }
  return `${def.hookName}: installed`;
}

function uninstallHook(hooksDir: string, def: HookDef): string {
  const hookPath = hookPathOrNull(hooksDir, def.hookName);
  if (!hookPath) return `${def.hookName}: skipped (unsafe hook name)`;
  if (!fs.existsSync(hookPath)) {
    return `${def.hookName}: not found`;
  }

  let content = fs.readFileSync(hookPath, 'utf-8');
  if (!content.includes(def.startMarker) && !hasLegacyMarker(content, def)) {
    return `${def.hookName}: not installed`;
  }

  content = stripMarkerSection(content, def.startMarker, def.endMarker);
  content = stripAllLegacySections(content, def);

  if (isOwnShebangOnly(content)) {
    fs.unlinkSync(hookPath);
    return `${def.hookName}: removed (deleted empty hook)`;
  }

  writeTextAtomic(hookPath, content);
  return `${def.hookName}: removed`;
}

function installGitHooksRaw(projectDir: string, entry: string): string[] {
  const gitRoot = getGitRoot(projectDir);
  if (!gitRoot) return ['Not a git repository'];

  const hooksDir = getHooksDir(gitRoot);
  return HOOK_DEFS.map(def => installHook(hooksDir, def, entry));
}

function uninstallGitHooksRaw(projectDir: string): string[] {
  const gitRoot = getGitRoot(projectDir);
  if (!gitRoot) return ['Not a git repository'];

  const hooksDir = getHooksDir(gitRoot);
  return HOOK_DEFS.map(def => uninstallHook(hooksDir, def));
}

export function statusGitHooks(projectDir: string): string[] {
  const gitRoot = getGitRoot(projectDir);
  if (!gitRoot) return ['Not a git repository'];

  const hooksDir = getHooksDir(gitRoot);
  const results: string[] = [];

  for (const def of HOOK_DEFS) {
    const hookPath = hookPathOrNull(hooksDir, def.hookName);
    if (!hookPath) {
      results.push(`${def.hookName}: skipped (unsafe hook name)`);
      continue;
    }
    if (fs.existsSync(hookPath)) {
      const content = fs.readFileSync(hookPath, 'utf-8');
      if (content.includes(def.startMarker)) {
        const recorded = recordedEntry(content);
        if (recorded !== null && !fs.existsSync(recorded)) {
          results.push(`${def.hookName}: installed, but its CLI is gone - run hook install to re-link it`);
        } else {
          results.push(`${def.hookName}: installed`);
        }
      } else if (hasLegacyMarker(content, def)) {
        results.push(`${def.hookName}: installed (legacy format - run hook install to migrate)`);
      } else {
        results.push(`${def.hookName}: not installed`);
      }
    } else {
      results.push(`${def.hookName}: not installed`);
    }
  }

  return results;
}

// Kept for reference in tests: the legacy marker families this installer
// recognizes (nodesify-graphify JS hooks, and pre-0.3 shell hooks).
export { LEGACY_HOOK_PREFIXES };

/// Install the hooks for the repository around `projectDir`, recording
/// `entry` as the CLI they run: by default this CLI, see `cliEntry`.
export function installGitHooks(projectDir: string, entry: string = cliEntry()): string[] { return withInstallLock(() => installGitHooksRaw(projectDir, entry)); }
export function uninstallGitHooks(projectDir: string): string[] { return withInstallLock(() => uninstallGitHooksRaw(projectDir)); }
