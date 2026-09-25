"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.LEGACY_HOOK_PREFIXES = void 0;
exports.installGitHooks = installGitHooks;
exports.uninstallGitHooks = uninstallGitHooks;
exports.statusGitHooks = statusGitHooks;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const child_process_1 = require("child_process");
const UPDATE_HELPER = `
const ASTRIA_HOOK_VERSION = '3';
// Prefer a workspace-local CLI, then a locally-installed package, and only
// then whatever is on PATH — a stale global install would rebuild the graph
// with old pipeline code and silently regress the report.
function runAstriaUpdate() {
  if (existsSync(path.join('packages', 'astria-cli', 'dist', 'index.js'))) {
    execSync('node packages/astria-cli/dist/index.js update .', { stdio: 'inherit' });
    return;
  }
  try {
    execSync('npx --no-install astria update .', { stdio: 'inherit' });
  } catch {
    execSync('astria update .', { stdio: 'inherit' });
  }
}
`;
const POST_COMMIT_SCRIPT = `// astria-hook-start
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
  if (hasCode && existsSync('.astria')) {
    runAstriaUpdate();
  }
} catch {}
// astria-hook-end
`;
const POST_CHECKOUT_SCRIPT = `// astria-checkout-hook-start
const { execSync } = require('child_process');
const { existsSync } = require('fs');
const path = require('path');
${UPDATE_HELPER}
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
const LEGACY_HOOK_PREFIXES = [
    { js: 'nodesify-graphify' }, // 0.3–0.9 JS-format hooks
    { shell: 'nodesify-graphify' }, // pre-0.3 shell-format hooks
];
exports.LEGACY_HOOK_PREFIXES = LEGACY_HOOK_PREFIXES;
const HOOK_DEFS = [
    {
        hookName: 'post-commit',
        script: POST_COMMIT_SCRIPT,
        startMarker: '// astria-hook-start',
        endMarker: '// astria-hook-end',
        legacyMarkers: [
            { startMarker: '// nodesify-graphify-hook-start', endMarker: '// nodesify-graphify-hook-end' },
            { startMarker: '# nodesify-graphify-hook-start', endMarker: '# nodesify-graphify-hook-end' },
        ],
    },
    {
        hookName: 'post-checkout',
        script: POST_CHECKOUT_SCRIPT,
        startMarker: '// astria-checkout-hook-start',
        endMarker: '// astria-checkout-hook-end',
        legacyMarkers: [
            { startMarker: '// nodesify-graphify-checkout-hook-start', endMarker: '// nodesify-graphify-checkout-hook-end' },
            { startMarker: '# nodesify-graphify-checkout-hook-start', endMarker: '# nodesify-graphify-checkout-hook-end' },
        ],
    },
];
const SHEBANGS = ['#!/bin/sh', '#!/bin/bash', '#!/usr/bin/env node'];
function escapeRegExp(s) {
    return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
function stripMarkerSection(content, startMarker, endMarker) {
    const regex = new RegExp('\\n*' + escapeRegExp(startMarker) + '[\\s\\S]*?' + escapeRegExp(endMarker) + '\\n*', 'g');
    return content.replace(regex, '\n');
}
function stripAllLegacySections(content, def) {
    for (const legacy of def.legacyMarkers) {
        content = stripMarkerSection(content, legacy.startMarker, legacy.endMarker);
    }
    return content;
}
function hasLegacyMarker(content, def) {
    return def.legacyMarkers.some((l) => content.includes(l.startMarker));
}
function isOwnShebangOnly(content) {
    const trimmed = content.trim();
    return trimmed === '' || SHEBANGS.includes(trimmed);
}
function getGitRoot(projectDir) {
    try {
        const result = (0, child_process_1.execSync)('git rev-parse --show-toplevel', {
            cwd: projectDir,
            encoding: 'utf-8',
        }).trim();
        return result;
    }
    catch {
        return null;
    }
}
function getHooksDir(gitRoot) {
    try {
        const customPath = (0, child_process_1.execSync)('git config core.hooksPath', {
            cwd: gitRoot,
            encoding: 'utf-8',
        }).trim();
        if (customPath) {
            return path.isAbsolute(customPath) ? customPath : path.join(gitRoot, customPath);
        }
    }
    catch {
        // no custom hooks path
    }
    return path.join(gitRoot, '.git', 'hooks');
}
/// Git hook names come from the fixed HOOK_DEFS allowlist; guard the join
/// anyway so no runtime-influenced name can reach the hooks directory path.
function safeHookName(name) {
    return /^[a-zA-Z0-9._-]+$/.test(name);
}
function hookPathOrNull(hooksDir, hookName) {
    if (!safeHookName(hookName))
        return null;
    return path.join(hooksDir, hookName);
}
function installHook(hooksDir, def) {
    const hookPath = hookPathOrNull(hooksDir, def.hookName);
    if (!hookPath)
        return `${def.hookName}: skipped (unsafe hook name)`;
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
            fs.writeFileSync(hookPath, '#!/usr/bin/env node\n\n' + def.script, 'utf-8');
            return hadLegacy
                ? `${def.hookName}: migrated legacy hook to current format`
                : `${def.hookName}: installed`;
        }
        if (content.includes(def.startMarker)) {
            // Refresh the script body when it predates the current template
            // (sentinel: ASTRIA_HOOK_VERSION resolver). Without this, fixed
            // templates would never reach already-installed hooks.
            if (!content.includes("ASTRIA_HOOK_VERSION = '3'")) {
                content = stripMarkerSection(content, def.startMarker, def.endMarker);
                const refreshed = content.trim() === '' || SHEBANGS.includes(content.trim())
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
    try {
        fs.chmodSync(hookPath, 0o755);
    }
    catch { /* Windows */ }
    return `${def.hookName}: installed`;
}
function uninstallHook(hooksDir, def) {
    const hookPath = hookPathOrNull(hooksDir, def.hookName);
    if (!hookPath)
        return `${def.hookName}: skipped (unsafe hook name)`;
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
    fs.writeFileSync(hookPath, content, 'utf-8');
    return `${def.hookName}: removed`;
}
function installGitHooks(projectDir) {
    const gitRoot = getGitRoot(projectDir);
    if (!gitRoot)
        return ['Not a git repository'];
    const hooksDir = getHooksDir(gitRoot);
    return HOOK_DEFS.map(def => installHook(hooksDir, def));
}
function uninstallGitHooks(projectDir) {
    const gitRoot = getGitRoot(projectDir);
    if (!gitRoot)
        return ['Not a git repository'];
    const hooksDir = getHooksDir(gitRoot);
    return HOOK_DEFS.map(def => uninstallHook(hooksDir, def));
}
function statusGitHooks(projectDir) {
    const gitRoot = getGitRoot(projectDir);
    if (!gitRoot)
        return ['Not a git repository'];
    const hooksDir = getHooksDir(gitRoot);
    const results = [];
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
            }
            else if (hasLegacyMarker(content, def)) {
                results.push(`${def.hookName}: installed (legacy format - run hook install to migrate)`);
            }
            else {
                results.push(`${def.hookName}: not installed`);
            }
        }
        else {
            results.push(`${def.hookName}: not installed`);
        }
    }
    return results;
}
//# sourceMappingURL=hooks.js.map