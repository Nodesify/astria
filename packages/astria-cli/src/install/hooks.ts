import * as fs from 'fs';
import * as path from 'path';
import { execSync } from 'child_process';

const UPDATE_HELPER = `
const GRAPHIFY_HOOK_VERSION = '2';
// Prefer a workspace-local CLI, then a locally-installed package, and only
// then whatever is on PATH — a stale global install would rebuild the graph
// with old pipeline code and silently regress the report.
function runGraphifyUpdate() {
  if (existsSync(path.join('packages', 'graphify-cli', 'dist', 'index.js'))) {
    execSync('node packages/graphify-cli/dist/index.js update .', { stdio: 'inherit' });
    return;
  }
  try {
    execSync('npx --no-install nodesify-graphify update .', { stdio: 'inherit' });
  } catch {
    execSync('nodesify-graphify update .', { stdio: 'inherit' });
  }
}
`;

const POST_COMMIT_SCRIPT = `// nodesify-graphify-hook-start
const { execSync } = require('child_process');
const { existsSync } = require('fs');
const path = require('path');
${UPDATE_HELPER}
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

  const codeExts = new Set(['.py', '.js', '.ts', '.tsx', '.jsx', '.rs', '.go', '.java', '.c', '.h', '.cpp', '.cc', '.cxx', '.hpp']);
  const hasCode = changed.split(/\\r?\\n/).some(f => codeExts.has(path.extname(f)));
  if (hasCode && existsSync('.graphify')) {
    runGraphifyUpdate();
  }
} catch {}
// nodesify-graphify-hook-end
`;

const POST_CHECKOUT_SCRIPT = `// nodesify-graphify-checkout-hook-start
const { execSync } = require('child_process');
const { existsSync } = require('fs');
const path = require('path');
${UPDATE_HELPER}
const branchSwitch = process.argv[3];
if (branchSwitch !== '1') process.exit(0);
if (!existsSync('.graphify')) process.exit(0);

try {
  // No shell redirects — see the note in the post-commit script.
  const gitDir = execSync('git rev-parse --git-dir', { encoding: 'utf-8' }).trim();
  if (existsSync(path.join(gitDir, 'rebase-merge')) || existsSync(path.join(gitDir, 'rebase-apply'))) process.exit(0);

  console.log('[nodesify-graphify] Branch switched - rebuilding knowledge graph...');
  runGraphifyUpdate();
} catch {}
// nodesify-graphify-checkout-hook-end
`;

interface HookDef {
  hookName: string;
  script: string;
  startMarker: string;
  endMarker: string;
  // Releases before 0.3.0 wrote shell-format hooks with '#' markers. The
  // current JS-format installer must recognize and replace them, otherwise
  // it appends JS to a #!/bin/sh file and every hook invocation errors.
  legacyStartMarker: string;
  legacyEndMarker: string;
}

const HOOK_DEFS: HookDef[] = [
  {
    hookName: 'post-commit',
    script: POST_COMMIT_SCRIPT,
    startMarker: '// nodesify-graphify-hook-start',
    endMarker: '// nodesify-graphify-hook-end',
    legacyStartMarker: '# nodesify-graphify-hook-start',
    legacyEndMarker: '# nodesify-graphify-hook-end',
  },
  {
    hookName: 'post-checkout',
    script: POST_CHECKOUT_SCRIPT,
    startMarker: '// nodesify-graphify-checkout-hook-start',
    endMarker: '// nodesify-graphify-checkout-hook-end',
    legacyStartMarker: '# nodesify-graphify-checkout-hook-start',
    legacyEndMarker: '# nodesify-graphify-checkout-hook-end',
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

function isOwnShebangOnly(content: string): boolean {
  const trimmed = content.trim();
  return trimmed === '' || SHEBANGS.includes(trimmed);
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

function installHook(hooksDir: string, def: HookDef): string {
  const hookPath = hookPathOrNull(hooksDir, def.hookName);
  if (!hookPath) return `${def.hookName}: skipped (unsafe hook name)`;
  if (!fs.existsSync(hooksDir)) {
    fs.mkdirSync(hooksDir, { recursive: true });
  }

  if (fs.existsSync(hookPath)) {
    let content = fs.readFileSync(hookPath, 'utf-8');
    const hadLegacy = content.includes(def.legacyStartMarker);
    if (hadLegacy) {
      content = stripMarkerSection(content, def.legacyStartMarker, def.legacyEndMarker);
    }

    if (isOwnShebangOnly(content)) {
      // File contained only our legacy section - rewrite fresh in current format
      fs.writeFileSync(hookPath, '#!/usr/bin/env node\n\n' + def.script, 'utf-8');
      return hadLegacy
        ? `${def.hookName}: migrated legacy hook to current format`
        : `${def.hookName}: installed`;
    }

    if (content.includes(def.startMarker)) {
      // Refresh the script body when it predates the current template
      // (sentinel: runGraphifyUpdate resolver). Without this, fixed
      // templates would never reach already-installed hooks.
      if (!content.includes("GRAPHIFY_HOOK_VERSION = '2'")) {
        content = stripMarkerSection(content, def.startMarker, def.endMarker);
        const refreshed =
          content.trim() === '' || SHEBANGS.includes(content.trim())
            ? '#!/usr/bin/env node\n\n' + def.script
            : content.trimEnd() + '\n\n' + def.script;
        fs.writeFileSync(hookPath, refreshed, 'utf-8');
        return `${def.hookName}: updated script to current version`;
      }
      if (hadLegacy) {
        fs.writeFileSync(hookPath, content, 'utf-8');
        return `${def.hookName}: already installed (stale legacy section removed)`;
      }
      return `${def.hookName}: already installed`;
    }

    fs.writeFileSync(hookPath, content.trimEnd() + '\n\n' + def.script, 'utf-8');
    return hadLegacy
      ? `${def.hookName}: appended to existing hook (replaced legacy section)`
      : `${def.hookName}: appended to existing hook`;
  }

  fs.writeFileSync(hookPath, '#!/usr/bin/env node\n\n' + def.script, 'utf-8');
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
  if (!content.includes(def.startMarker) && !content.includes(def.legacyStartMarker)) {
    return `${def.hookName}: not installed`;
  }

  content = stripMarkerSection(content, def.startMarker, def.endMarker);
  content = stripMarkerSection(content, def.legacyStartMarker, def.legacyEndMarker);

  if (isOwnShebangOnly(content)) {
    fs.unlinkSync(hookPath);
    return `${def.hookName}: removed (deleted empty hook)`;
  }

  fs.writeFileSync(hookPath, content, 'utf-8');
  return `${def.hookName}: removed`;
}

export function installGitHooks(projectDir: string): string[] {
  const gitRoot = getGitRoot(projectDir);
  if (!gitRoot) return ['Not a git repository'];

  const hooksDir = getHooksDir(gitRoot);
  return HOOK_DEFS.map(def => installHook(hooksDir, def));
}

export function uninstallGitHooks(projectDir: string): string[] {
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
        results.push(`${def.hookName}: installed`);
      } else if (content.includes(def.legacyStartMarker)) {
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
