import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { createHash } from 'crypto';
import { withInstallLock, writeTextAtomic } from './atomic';
import { InstallScope, installRoot, readInstallState, saveInstallState } from './state';
import { PLATFORMS, PLATFORM_NAMES, PlatformConfig } from './platforms';
import { injectSection, removeSection, PROJECT_MD_SECTION, SKILL_REGISTRATION, SectionResult } from './markdown-inject';
import {
  injectCodexMcp, removeCodexMcp,
  generatedIntegrationFiles,
  injectClaudeHook, removeClaudeHook,
  injectCodexHook, removeCodexHook,
  injectGeminiHook, removeGeminiHook,
  injectOpenCodePlugin, removeOpenCodePlugin,
  injectPiExtension, removePiExtension,
  injectCursorRule, removeCursorRule,
  injectKiroSteering, removeKiroSteering,
  injectAgentMcp, removeAgentMcp, McpFlavor, cleanupLegacyCopilotMcp,
} from './settings-inject';

function sectionMessage(result: SectionResult, added: string, updated: string, unchanged: string): string {
  return result === 'added' ? added : result === 'updated' ? updated : unchanged;
}

const MCP_LABELS: Record<McpFlavor, { name: string; file: string }> = {
  zcode: { name: 'ZCode MCP server', file: '.zcode/config.json' },
  claude: { name: 'Claude MCP server', file: '.mcp.json' },
  cursor: { name: 'Cursor MCP server', file: '.cursor/mcp.json' },
  gemini: { name: 'Gemini MCP server', file: '.gemini/settings.json' },
  vscode: { name: 'VS Code MCP server', file: '.vscode/mcp.json' },
  trae: { name: 'Trae MCP server', file: '.trae/mcp.json' },
  windsurf: { name: 'Windsurf MCP server', file: '.windsurf/mcp.json' },
  kiro: { name: 'Kiro MCP server', file: '.kiro/settings/mcp.json' },
  opencode: { name: 'OpenCode MCP server', file: '.opencode/opencode.json' },
  pi: { name: 'astria MCP server (pi-mcp-adapter reads it)', file: '.mcp.json' },
  codex: { name: 'Codex MCP server', file: '.codex/config.toml' },
};

function getSkillDir(): string {
  return path.resolve(__dirname, '..', '..', 'skills');
}

function skillDstPath(cfg: PlatformConfig, projectDir: string): string {
  return path.join(projectDir, cfg.skillDst);
}

function removeFile(file: string): string {
  try { fs.unlinkSync(file); return `Removed: ${file}`; }
  catch (error: any) { if (error.code === 'ENOENT') return `Not found: ${file}`; throw error; }
}

function generatedFiles(platform: string, root: string, scope: InstallScope, requireSource = true): Record<string, string> {
  const cfg = PLATFORMS[platform];
  const files = generatedIntegrationFiles(root, platform, scope === 'user');
  if (cfg.skillFile) {
    try { files[skillDstPath(cfg, root)] = fs.readFileSync(path.join(getSkillDir(), cfg.skillFile), 'utf8'); }
    catch (error: any) { if (requireSource || error.code !== 'ENOENT') throw error; files[skillDstPath(cfg, root)] = ''; }
  }
  return files;
}

const fingerprint = (text: string) => createHash('sha256').update(text).digest('hex');
function checkOwnedFiles(files: Record<string, string>, recorded: Record<string, string>): void {
  for (const [file, expected] of Object.entries(files)) {
    let current: string;
    try { current = fs.readFileSync(file, 'utf8'); }
    catch (error: any) { if (error.code === 'ENOENT') continue; throw error; }
    if (fingerprint(current) !== (recorded[file] ?? fingerprint(expected))) {
      throw new Error(`Customized file retained: ${file}. Back it up and move it out of the managed location before retrying.`);
    }
  }
}

function userIntegration(platform: string, root: string, remove: boolean, remaining: string[]): string[] {
  const cfg = PLATFORMS[platform];
  const messages: string[] = [];
  if (!cfg.skillFile && platform !== 'pi' && platform !== 'codex') {
    throw new Error(`${platform} has no user-scoped integration; use --scope project.`);
  }
  if (cfg.skillFile && !remaining.some(p => PLATFORMS[p]?.skillDst === cfg.skillDst)) {
    messages.push(...(remove ? [removeFile(skillDstPath(cfg, root))] : copySkillFile(platform, cfg, root)));
  }
  if (platform === 'claude') {
    const file = path.join(root, '.claude', 'CLAUDE.md');
    if (remove) removeSection(file); else injectSection(file, SKILL_REGISTRATION);
    messages.push(`User Claude registration: ${remove ? 'removed' : 'installed'}`);
  }
  if (platform === 'codex') {
    const changed = remove ? removeCodexMcp(root) : injectCodexMcp(root);
    messages.push(`User Codex MCP: ${changed ? (remove ? 'removed' : 'installed') : 'unchanged'}`);
  }
  if (platform === 'pi') {
    const changed = remove ? removePiExtension() : injectPiExtension();
    messages.push(`User Pi extension: ${changed ? (remove ? 'removed' : 'installed') : 'unchanged'}`);
  }
  return messages;
}

export function installPlatform(platform: string, projectDir: string, scope: InstallScope = 'project'): string[] {
  if (!PLATFORMS[platform]) throw new Error(`Unknown platform: ${platform}`);
  if (scope === 'user' && ((!PLATFORMS[platform].skillFile && platform !== 'pi') || PLATFORMS[platform].skillScope === 'project')) throw new Error(`${platform} has no user-scoped integration; use --scope project.`);
  return withInstallLock(() => {
    const state = readInstallState(projectDir, scope);
    const root = installRoot(projectDir, scope);
    const files = generatedFiles(platform, root, scope);
    checkOwnedFiles(files, state.files);
    // Record before writing: a partial install remains discoverable and removable.
    if (!state.platforms.includes(platform)) state.platforms.push(platform);
    saveInstallState(projectDir, scope, state);
    try {
      const messages = scope === 'user' ? userIntegration(platform, root, false, []) : installPlatformRaw(platform, root);
      return [`Scope: ${scope} (${root})`, ...messages];
    } finally {
      // Preserve old fingerprints after an interrupted upgrade; capture only files actually written.
      for (const [file, content] of Object.entries(files)) {
        try { if (fs.readFileSync(file, 'utf8') === content) state.files[file] = fingerprint(content); }
        catch (error: any) { if (error.code !== 'ENOENT') throw error; }
      }
      saveInstallState(projectDir, scope, state);
    }
  });
}

export function uninstallPlatform(platform: string, projectDir: string, scope: InstallScope = 'project'): string[] {
  if (!PLATFORMS[platform]) throw new Error(`Unknown platform: ${platform}`);
  if (scope === 'user' && ((!PLATFORMS[platform].skillFile && platform !== 'pi') || PLATFORMS[platform].skillScope === 'project')) throw new Error(`${platform} has no user-scoped integration; use --scope project.`);
  return withInstallLock(() => {
    const state = readInstallState(projectDir, scope);
    const remaining = state.platforms.filter(p => p !== platform);
    const root = installRoot(projectDir, scope);
    const files = generatedFiles(platform, root, scope, false);
    checkOwnedFiles(files, state.files);
    const messages = scope === 'user' ? userIntegration(platform, root, true, remaining) : uninstallPlatformRaw(platform, root, remaining);
    state.platforms = remaining;
    const retained = new Set(remaining.flatMap(p => Object.keys(generatedFiles(p, root, scope, false))));
    for (const file of Object.keys(files)) if (!retained.has(file)) delete state.files[file];
    saveInstallState(projectDir, scope, state);
    return [`Scope: ${scope} (${root})`, ...messages];
  });
}

/** Explicit, independent data deletion; integration removal does not delete data. */
export function purgeData(projectDir: string, global: boolean): string[] {
  return withInstallLock(() => {
    const target = path.resolve(global ? path.join(os.homedir(), '.astria') : path.join(projectDir, '.astria'));
    const root = path.resolve(global ? os.homedir() : projectDir);
    if (path.dirname(target) !== root || path.basename(target) !== '.astria') throw new Error('Unsafe purge target.');
    if (!fs.existsSync(target)) return [`Not found: ${target}`];
    if (fs.lstatSync(target).isSymbolicLink()) throw new Error(`Refusing to purge a linked directory: ${target}`);
    fs.rmSync(target, { recursive: true });
    return [`Removed ${global ? 'global store' : 'project graph data'}: ${target}`];
  });
}

function copySkillFile(platform: string, cfg: PlatformConfig, projectDir: string): string[] {
  const messages: string[] = [];
  if (!cfg.skillFile) return messages;

  // skillDst comes from the static platform table; refuse traversal
  // segments anyway so no runtime value can redirect the install target.
  if (cfg.skillDst.split(/[\\/]/).includes('..')) {
    throw new Error(`Unsafe skill destination: ${cfg.skillDst}`);
  }

  const src = path.join(getSkillDir(), cfg.skillFile);
  if (!fs.existsSync(src)) {
    throw new Error(`Skill file not found: ${src}`);
  }

  const dst = skillDstPath(cfg, projectDir);
  copyFile(src, dst);
  messages.push(`Skill file -> ${dst}`);
  return messages;
}

function copyFile(src: string, dst: string) {
  // Sink guard: only normalized absolute destinations without traversal
  // segments may be written, regardless of how the caller derived them.
  if (!path.isAbsolute(dst) || dst.split(/[\\/]/).includes('..')) {
    throw new Error(`Unsafe destination: ${dst}`);
  }
  const dir = path.dirname(dst);
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
  writeTextAtomic(dst, fs.readFileSync(src, 'utf8'));
}

function installPlatformRaw(platform: string, projectDir: string): string[] {
  const messages: string[] = [];

  if (platform === 'cursor') {
    if (injectCursorRule(projectDir)) {
      messages.push('Cursor rule -> .cursor/rules/astria.mdc');
    } else {
      messages.push('Cursor rule: already installed');
    }
    messages.push(
      injectAgentMcp(projectDir, 'cursor')
        ? 'Cursor MCP server -> .cursor/mcp.json'
        : 'Cursor MCP server: already registered'
    );
    return messages;
  }

  if (platform === 'kiro') {
    const cfg = PLATFORMS.kiro;
    messages.push(...copySkillFile('kiro', cfg, projectDir));
    if (injectKiroSteering(projectDir)) {
      messages.push('Kiro steering -> .kiro/steering/astria.md');
    } else {
      messages.push('Kiro steering: already installed');
    }
    if (cfg.mcp) {
      const label = MCP_LABELS[cfg.mcp];
      messages.push(
        injectAgentMcp(projectDir, cfg.mcp)
          ? `${label.name} -> ${label.file}`
          : `${label.name}: already registered`
      );
    }
    return messages;
  }

  const cfg = PLATFORMS[platform];
  if (!cfg) {
    throw new Error(`Unknown platform: ${platform}. Available: ${Object.keys(PLATFORMS).join(', ')}`);
  }

  messages.push(...copySkillFile(platform, cfg, projectDir));

  const projectMd = path.join(projectDir, 'CLAUDE.md');
  if (cfg.claudeMd || cfg.agentsMd || cfg.geminiMd || cfg.copilotMd) {
    if (cfg.claudeMd) {
      const result = injectSection(projectMd, PROJECT_MD_SECTION);
      messages.push(
        sectionMessage(
          result,
          'Project CLAUDE.md: astria section added',
          'Project CLAUDE.md: astria section updated',
          'Project CLAUDE.md: astria section unchanged'
        )
      );
    }

    if (cfg.agentsMd) {
      const agentsMd = path.join(projectDir, 'AGENTS.md');
      const result = injectSection(agentsMd, PROJECT_MD_SECTION);
      messages.push(
        sectionMessage(
          result,
          'Project AGENTS.md: astria section added',
          'Project AGENTS.md: astria section updated',
          'Project AGENTS.md: astria section unchanged'
        )
      );
    }

    if (cfg.geminiMd) {
      const geminiMd = path.join(projectDir, 'GEMINI.md');
      const result = injectSection(geminiMd, PROJECT_MD_SECTION);
      messages.push(
        sectionMessage(
          result,
          'Project GEMINI.md: astria section added',
          'Project GEMINI.md: astria section updated',
          'Project GEMINI.md: astria section unchanged'
        )
      );
    }

    if (cfg.copilotMd) {
      // Copilot's custom-instructions file — same managed section, so a
      // later install refreshes it in place and uninstall removes it.
      const copilotMd = path.join(projectDir, '.github', 'copilot-instructions.md');
      const result = injectSection(copilotMd, PROJECT_MD_SECTION);
      messages.push(
        sectionMessage(
          result,
          'Copilot instructions -> .github/copilot-instructions.md',
          'Copilot instructions: section updated',
          'Copilot instructions: already up to date'
        )
      );
      // The Copilot coding agent reads MCP config only from repository
      // Settings (no committed file); a 1.0.9-era install wrote a dead
      // .github/copilot-mcp.json — clean it up.
      if (cleanupLegacyCopilotMcp(projectDir)) {
        messages.push('Legacy Copilot MCP config removed (.github/copilot-mcp.json is read by nothing)');
      }
    }
  }

  switch (cfg.settingsHook) {
    case 'claude':
      if (injectClaudeHook(projectDir)) {
        messages.push('Claude PreToolUse hook -> .claude/settings.json');
      } else {
        messages.push('Claude PreToolUse hook: already installed');
      }
      break;
    case 'codex':
      if (injectCodexHook(projectDir)) {
        messages.push('Codex PreToolUse hook -> .codex/hooks.json');
      } else {
        messages.push('Codex PreToolUse hook: already installed');
      }
      break;
    case 'gemini':
      if (injectGeminiHook(projectDir)) {
        messages.push('Gemini BeforeTool hook -> .gemini/settings.json');
      } else {
        messages.push('Gemini BeforeTool hook: already installed');
      }
      break;
    case 'pi':
      if (injectPiExtension(projectDir)) {
        messages.push('Pi extension -> .pi/extensions/astria.mjs');
      } else {
        messages.push('Pi extension: already installed');
      }
      break;
    case 'opencode':
      if (injectOpenCodePlugin(projectDir)) {
        messages.push('OpenCode plugin -> .opencode/plugins/astria.js');
      } else {
        messages.push('OpenCode plugin: already installed');
      }
      break;
  }

  if (cfg.mcp) {
    const label = MCP_LABELS[cfg.mcp];
    messages.push(
      injectAgentMcp(projectDir, cfg.mcp)
        ? `${label.name} -> ${label.file}`
        : `${label.name}: already registered`
    );
  }

  return messages;
}

function uninstallPlatformRaw(platform: string, projectDir: string, remaining: string[]): string[] {
  const messages: string[] = [];

  if (platform === 'cursor') {
    if (removeCursorRule(projectDir)) {
      messages.push('Cursor rule: removed');
    } else {
      messages.push('Cursor rule: not found');
    }
    messages.push(
      removeAgentMcp(projectDir, 'cursor')
        ? 'Cursor MCP server: removed'
        : 'Cursor MCP server: not found'
    );
    return messages;
  }

  if (platform === 'kiro') {
    const cfg = PLATFORMS.kiro;
    if (cfg.skillFile) {
      messages.push(removeFile(skillDstPath(cfg, projectDir)));
    }
    if (removeKiroSteering(projectDir)) {
      messages.push('Kiro steering: removed');
    } else {
      messages.push('Kiro steering: not found');
    }
    if (cfg.mcp) {
      messages.push(
        removeAgentMcp(projectDir, cfg.mcp)
          ? 'Kiro MCP server: removed'
          : 'Kiro MCP server: not found'
      );
    }
    return messages;
  }

  const cfg = PLATFORMS[platform];
  if (!cfg) {
    throw new Error(`Unknown platform: ${platform}. Available: ${Object.keys(PLATFORMS).join(', ')}`);
  }

  if (cfg.skillFile) {
    if (!remaining.some(p => PLATFORMS[p]?.skillDst === cfg.skillDst)) {
      messages.push(removeFile(skillDstPath(cfg, projectDir)));
    } else messages.push('Skill file retained for another platform');
  }

  if (cfg.claudeMd) {
    messages.push(removeSection(path.join(projectDir, 'CLAUDE.md')) ? 'Project CLAUDE.md: astria section removed' : 'Project CLAUDE.md: absent or customized');
  }
  if (cfg.agentsMd && !remaining.some(p => PLATFORMS[p]?.agentsMd)) {
    messages.push(removeSection(path.join(projectDir, 'AGENTS.md')) ? 'Project AGENTS.md: astria section removed' : 'Project AGENTS.md: absent or customized');
  }
  if (cfg.geminiMd) {
    messages.push(removeSection(path.join(projectDir, 'GEMINI.md')) ? 'Project GEMINI.md: astria section removed' : 'Project GEMINI.md: absent or customized');
  }
  if (cfg.copilotMd) {
    messages.push(removeSection(path.join(projectDir, '.github', 'copilot-instructions.md')) ? 'Copilot instructions: astria section removed' : 'Copilot instructions: absent or customized');
    if (cleanupLegacyCopilotMcp(projectDir)) {
      messages.push('Legacy Copilot MCP config removed (.github/copilot-mcp.json is read by nothing)');
    }
  }

  switch (cfg.settingsHook) {
    case 'claude':
      messages.push(removeClaudeHook(projectDir) ? 'Claude PreToolUse hook: removed' : 'Claude PreToolUse hook: absent or customized');
      break;
    case 'codex':
      messages.push(removeCodexHook(projectDir) ? 'Codex PreToolUse hook: removed' : 'Codex PreToolUse hook: absent or customized');
      break;
    case 'gemini':
      messages.push(removeGeminiHook(projectDir) ? 'Gemini BeforeTool hook: removed' : 'Gemini BeforeTool hook: absent or customized');
      break;
    case 'pi':
      messages.push(removePiExtension(projectDir) ? 'Pi extension: removed' : 'Pi extension: not found');
      break;
    case 'opencode':
      messages.push(removeOpenCodePlugin(projectDir) ? 'OpenCode plugin: removed' : 'OpenCode plugin: absent or customized');
      break;
  }

  if (cfg.mcp && !remaining.some(p => PLATFORMS[p]?.mcp === cfg.mcp || (['claude', 'pi'].includes(cfg.mcp!) && ['claude', 'pi'].includes(PLATFORMS[p]?.mcp ?? '')))) {
    const label = MCP_LABELS[cfg.mcp];
    messages.push(
      removeAgentMcp(projectDir, cfg.mcp)
        ? `${label.name}: removed`
        : `${label.name}: absent or customized`
    );
  }

  return messages;
}
