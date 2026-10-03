import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { PLATFORMS, PLATFORM_NAMES, PlatformConfig } from './platforms';
import { uninstallGitHooks } from './hooks';
import { mergeDriverUninstall } from '../commands/merge-driver';
import { injectSection, removeSection, PROJECT_MD_SECTION, SKILL_REGISTRATION, SectionResult } from './markdown-inject';
import {
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
  codex: { name: 'Codex MCP server', file: '~/.codex/config.toml (user-global)' },
};

function getSkillDir(): string {
  return path.resolve(__dirname, '..', '..', 'skills');
}

/// CLAUDE_CONFIG_DIR is read exactly once, at load, and sanitized to a
/// normalized absolute path without traversal segments (or undefined). No
/// other code path reads it, so env input can never reach a filesystem
/// write unvalidated.
const CLAUDE_CONFIG_DIR: string | undefined = (() => {
  const raw = process.env.CLAUDE_CONFIG_DIR;
  if (!raw) return undefined;
  const resolved = path.resolve(raw);
  return raw === resolved && !raw.includes('..') && path.isAbsolute(raw)
    ? resolved
    : undefined;
})();

/// Resolve the live skill destination for a platform config: home-scoped
/// platforms root at the user's home dir; project-scoped ones (Copilot)
/// root at the project.
function skillDstPath(cfg: PlatformConfig, projectDir: string): string {
  return cfg.skillScope === 'project'
    ? path.join(projectDir, cfg.skillDst)
    : path.join(os.homedir(), cfg.skillDst);
}

function copySkillFile(platform: string, cfg: PlatformConfig, projectDir: string): string[] {
  const messages: string[] = [];
  if (!cfg.skillFile) return messages;

  // skillDst comes from the static platform table; refuse traversal
  // segments anyway so no runtime value can redirect the install target.
  if (cfg.skillDst.split(/[\\/]/).includes('..')) {
    messages.push(`Refusing unsafe skill destination: ${cfg.skillDst}`);
    return messages;
  }

  const src = path.join(getSkillDir(), cfg.skillFile);
  if (!fs.existsSync(src)) {
    messages.push(`Skill file not found: ${src}`);
    return messages;
  }

  if (platform === 'claude' && CLAUDE_CONFIG_DIR) {
    const overrideDst = path.join(CLAUDE_CONFIG_DIR, 'skills', 'astria', 'SKILL.md');
    copyFile(src, overrideDst);
    messages.push(`Skill file -> ${overrideDst}`);
    return messages;
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
    return;
  }
  const dir = path.dirname(dst);
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
  fs.copyFileSync(src, dst);
}

/// Pre-1.0 installs wrote skills under `skills/graphify/`. The path carries
/// exactly one `astria` segment (the skill dir name), so the legacy
/// destination is derivable by swapping that segment. Layouts whose `astria`
/// is a file stem rather than a directory (`.clinerules/astria.md`) have no
/// legacy variant — the swap is a no-op there and must not run, or the
/// cleanup would delete the freshly installed skill.
function legacySkillDst(cfg: PlatformConfig): string {
  return cfg.skillDst.replace(/([\\/])astria([\\/])/, '$1graphify$2');
}

/// Removes stale skill files this installer no longer produces: the pre-1.0
/// graphify name, and — for project-scoped platforms (Copilot) — the
/// 1.0.9/1.0.10-era copy under the user's home directory, which nothing
/// reads. Now-empty folders are removed so no stale skill dir lingers.
/// Layouts with no legacy variant (file stems like .clinerules/astria.md,
/// where the graphify swap is a no-op) have nothing to clean — the identity
/// guard must hold or a second install would delete its own fresh skill.
function removeLegacySkillFile(platform: string, cfg: PlatformConfig, projectDir: string): string[] {
  const messages: string[] = [];
  if (!cfg.skillFile) return messages;

  const home = os.homedir();
  const hasLegacyVariant = legacySkillDst(cfg) !== cfg.skillDst;
  const targets: string[] = [];
  if (cfg.skillScope === 'project') {
    // Current name at the wrong (home) root, plus the legacy name at both
    // roots when one exists.
    targets.push(path.join(home, cfg.skillDst));
    if (hasLegacyVariant) {
      targets.push(path.join(home, legacySkillDst(cfg)), path.join(projectDir, legacySkillDst(cfg)));
    }
  } else if (hasLegacyVariant) {
    targets.push(path.join(home, legacySkillDst(cfg)));
  }
  if (platform === 'claude' && CLAUDE_CONFIG_DIR) {
    targets.push(path.join(CLAUDE_CONFIG_DIR, 'skills', 'graphify', 'SKILL.md'));
  }

  for (const legacy of targets) {
    const dir = path.dirname(legacy);
    const hadDir = fs.existsSync(dir);
    if (fs.existsSync(legacy)) {
      try {
        fs.unlinkSync(legacy);
        messages.push(`Legacy skill file removed: ${legacy}`);
      } catch { /* unreadable — leave it */ }
    }
    if (!hadDir) continue;
    // Install stamps from both eras; a folder holding only these is removed
    // outright so no stale skill directory lingers.
    for (const stamp of ['.astria_version', '.graphify_version']) {
      try { fs.unlinkSync(path.join(dir, stamp)); } catch { /* absent */ }
    }
    try { fs.rmdirSync(dir); } catch { /* not empty — leave */ }
  }
  return messages;
}

export function installPlatform(platform: string, projectDir: string): string[] {
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
    messages.push(...removeLegacySkillFile('kiro', cfg, projectDir));
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
  messages.push(...removeLegacySkillFile(platform, cfg, projectDir));

  if (cfg.claudeMd) {
    const claudeMdPath = path.join(os.homedir(), '.claude', 'CLAUDE.md');
    const registration = injectSection(claudeMdPath, SKILL_REGISTRATION);
    messages.push(
      sectionMessage(
        registration,
        'User CLAUDE.md: skill registration added',
        'User CLAUDE.md: skill registration updated',
        'User CLAUDE.md: already registered'
      )
    );
  }

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
      if (injectPiExtension()) {
        messages.push('Pi extension -> ~/.pi/agent/extensions/astria.mjs');
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

/// Deep clean (`uninstall --purge`): everything uninstall does, plus the
/// artifacts uninstall deliberately leaves alone — git hooks, the merge
/// driver wiring, the project's `.astria/` data directory, and the global
/// cross-repo store. Explicit-flag consent only; there is no interactive
/// prompt, so CI can run it unattended.
export function purgeEverything(projectDir: string): string[] {
  const messages: string[] = [];

  // 1. Every platform's skill/MCP/hook registrations.
  for (const platform of PLATFORM_NAMES) {
    try {
      messages.push(...uninstallPlatform(platform, projectDir));
    } catch (e: any) {
      messages.push(`${platform}: ${e.message || e}`);
    }
  }

  // 2. Git post-commit / post-checkout hooks installed by `astria hook install`.
  try {
    messages.push(...uninstallGitHooks(projectDir));
  } catch (e: any) {
    messages.push(`git hooks: ${e.message || e}`);
  }

  // 3. Merge-driver wiring from `astria merge-driver install`.
  try {
    messages.push(...mergeDriverUninstall(projectDir));
  } catch (e: any) {
    messages.push(`merge driver: ${e.message || e}`);
  }

  // 4. The project's graph data: graphs, wiki, transcripts, caches,
  //    history — everything lives under .astria/.
  const projectData = path.join(projectDir, '.astria');
  if (fs.existsSync(projectData)) {
    try {
      fs.rmSync(projectData, { recursive: true, force: true });
      messages.push(`Project graph data removed: ${projectData}`);
    } catch (e: any) {
      messages.push(`Project graph data (${projectData}): ${e.message || e}`);
    }
  } else {
    messages.push('Project graph data: not found');
  }

  // 5. The global cross-repo store (~/.astria/global.db and friends).
  const globalDir = path.join(os.homedir(), '.astria');
  if (fs.existsSync(globalDir)) {
    try {
      fs.rmSync(globalDir, { recursive: true, force: true });
      messages.push(`Global store removed: ${globalDir}`);
    } catch (e: any) {
      messages.push(`Global store (${globalDir}): ${e.message || e}`);
    }
  } else {
    messages.push('Global store: not found');
  }

  return messages;
}

export function uninstallPlatform(platform: string, projectDir: string): string[] {
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
      try { fs.unlinkSync(skillDstPath(cfg, projectDir)); messages.push(`Skill file removed: ${skillDstPath(cfg, projectDir)}`); } catch { messages.push('Skill file: not found'); }
    }
    messages.push(...removeLegacySkillFile('kiro', cfg, projectDir));
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
    // The live location by scope, plus — for claude — both candidate roots,
    // because CLAUDE_CONFIG_DIR may have been set at install time but not
    // now (or vice versa). Uninstall uses the same sanitized const install
    // reads; the raw env var never reaches a filesystem delete.
    const candidates = [skillDstPath(cfg, projectDir)];
    if (platform === 'claude' && CLAUDE_CONFIG_DIR) {
      candidates.push(path.join(CLAUDE_CONFIG_DIR, 'skills', 'astria', 'SKILL.md'));
    }
    let removedAny = false;
    for (const dst of candidates) {
      try { fs.unlinkSync(dst); messages.push(`Skill file removed: ${dst}`); removedAny = true; } catch { /* absent */ }
    }
    if (!removedAny) messages.push('Skill file: not found');
  }
  messages.push(...removeLegacySkillFile(platform, cfg, projectDir));

  if (cfg.claudeMd) {
    const claudeMdPath = path.join(os.homedir(), '.claude', 'CLAUDE.md');
    removeSection(claudeMdPath);
    messages.push('User CLAUDE.md: astria section removed');
  }

  if (cfg.claudeMd) {
    removeSection(path.join(projectDir, 'CLAUDE.md'));
    messages.push('Project CLAUDE.md: astria section removed');
  }
  if (cfg.agentsMd) {
    removeSection(path.join(projectDir, 'AGENTS.md'));
    messages.push('Project AGENTS.md: astria section removed');
  }
  if (cfg.geminiMd) {
    removeSection(path.join(projectDir, 'GEMINI.md'));
    messages.push('Project GEMINI.md: astria section removed');
  }
  if (cfg.copilotMd) {
    removeSection(path.join(projectDir, '.github', 'copilot-instructions.md'));
    messages.push('Copilot instructions: astria section removed');
    if (cleanupLegacyCopilotMcp(projectDir)) {
      messages.push('Legacy Copilot MCP config removed (.github/copilot-mcp.json is read by nothing)');
    }
  }

  switch (cfg.settingsHook) {
    case 'claude':
      removeClaudeHook(projectDir);
      messages.push('Claude PreToolUse hook: removed');
      break;
    case 'codex':
      removeCodexHook(projectDir);
      messages.push('Codex PreToolUse hook: removed');
      break;
    case 'gemini':
      removeGeminiHook(projectDir);
      messages.push('Gemini BeforeTool hook: removed');
      break;
    case 'opencode':
      removeOpenCodePlugin(projectDir);
      messages.push('OpenCode plugin: removed');
      break;
  }

  if (cfg.mcp) {
    const label = MCP_LABELS[cfg.mcp];
    messages.push(
      removeAgentMcp(projectDir, cfg.mcp)
        ? `${label.name}: removed`
        : `${label.name}: not found`
    );
  }

  return messages;
}
