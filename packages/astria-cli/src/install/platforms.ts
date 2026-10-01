import * as path from 'path';
import * as os from 'os';
import type { McpFlavor } from './settings-inject';

export interface PlatformConfig {
  skillFile: string;
  skillDst: string;
  /** Where skillDst is rooted: the user's home dir (default) or the
   * project directory (Copilot reads repo-scoped .github/skills/). */
  skillScope?: 'home' | 'project';
  claudeMd: boolean;
  agentsMd: boolean;
  geminiMd: boolean;
  /** Inject the managed section into `<project>/.github/copilot-instructions.md`. */
  copilotMd?: boolean;
  settingsHook: 'claude' | 'codex' | 'gemini' | 'opencode' | 'pi' | 'none';
  /** Register the astria MCP server in this platform's config ('codex' is
   * user-global TOML; the JSON flavors are project-scoped). */
  mcp?: McpFlavor;
}

export const PLATFORMS: Record<string, PlatformConfig> = {
  claude: {
    skillFile: 'skill.md',
    skillDst: path.join('.claude', 'skills', 'astria', 'SKILL.md'),
    claudeMd: true,
    agentsMd: false,
    geminiMd: false,
    settingsHook: 'claude',
    mcp: 'claude',
  },
  codex: {
    skillFile: 'skill-codex.md',
    skillDst: path.join('.agents', 'skills', 'astria', 'SKILL.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'codex',
    mcp: 'codex',
  },
  gemini: {
    skillFile: 'skill-gemini.md',
    skillDst: os.platform() === 'win32'
      ? path.join('.agents', 'skills', 'astria', 'SKILL.md')
      : path.join('.gemini', 'skills', 'astria', 'SKILL.md'),
    claudeMd: false,
    agentsMd: false,
    geminiMd: true,
    settingsHook: 'gemini',
    mcp: 'gemini',
  },
  opencode: {
    skillFile: 'skill-opencode.md',
    skillDst: path.join('.config', 'opencode', 'skills', 'astria', 'SKILL.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'opencode',
    mcp: 'opencode',
  },
  cursor: {
    skillFile: '',
    skillDst: '',
    claudeMd: false,
    agentsMd: false,
    geminiMd: false,
    settingsHook: 'none',
    mcp: 'cursor',
  },
  kiro: {
    skillFile: 'skill.md',
    skillDst: path.join('.kiro', 'skills', 'astria', 'SKILL.md'),
    claudeMd: false,
    agentsMd: false,
    geminiMd: false,
    settingsHook: 'none',
    mcp: 'kiro',
  },
  aider: {
    skillFile: 'skill-aider.md',
    skillDst: path.join('.aider', 'skills', 'astria', 'SKILL.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'none',
  },
  copilot: {
    skillFile: 'skill-copilot.md',
    // Repo-scoped: the Copilot coding agent reads .github/skills/ from the
    // repository it works on (docs.github.com "Copilot coding agent skills"),
    // not from the user's home directory. 1.0.9/1.0.10 installs wrote this
    // under ~ — removeLegacySkillFile cleans that copy up on the next
    // install or uninstall.
    skillDst: path.join('.github', 'skills', 'astria', 'SKILL.md'),
    skillScope: 'project',
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    // Copilot reads custom instructions from .github/copilot-instructions.md;
    // AGENTS.md support varies by Copilot version, so write both.
    copilotMd: true,
    settingsHook: 'none',
    // No MCP registration: the Copilot coding agent has no committed repo
    // config file — repository-level MCP is JSON pasted into the repository
    // Settings UI (docs.github.com). The dead .github/copilot-mcp.json a
    // 1.0.9-era install wrote is cleaned up on install/uninstall (VS Code
    // users get MCP via the separate vscode platform).
  },
  trae: {
    skillFile: 'skill-trae.md',
    skillDst: path.join('.trae', 'skills', 'astria', 'SKILL.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'none',
    mcp: 'trae',
  },
  zcode: {
    // skill-codex.md is CLI-oriented, which is what ZCode sessions use
    // unless the astria MCP server is registered by the mcp flag below.
    skillFile: 'skill-codex.md',
    skillDst: path.join('.zcode', 'skills', 'astria', 'SKILL.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'none',
    mcp: 'zcode',
  },
  // VS Code native workspace MCP — covers every editor based on it,
  // including GitHub Copilot inside VS Code. No skill file mechanism:
  // VS Code agents read workspace instructions via other platforms' files.
  vscode: {
    skillFile: '',
    skillDst: '',
    claudeMd: false,
    agentsMd: false,
    geminiMd: false,
    settingsHook: 'none',
    mcp: 'vscode',
  },
  windsurf: {
    skillFile: '',
    skillDst: '',
    claudeMd: false,
    agentsMd: false,
    geminiMd: false,
    settingsHook: 'none',
    mcp: 'windsurf',
  },
  // Cline workspace rules (~/.clinerules) — CLI-oriented skill, same as codex.
  cline: {
    skillFile: 'skill-codex.md',
    skillDst: path.join('.clinerules', 'astria.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'none',
  },
  // Roo Code global rules directory.
  roo: {
    skillFile: 'skill-codex.md',
    skillDst: path.join('.roo', 'rules', 'astria.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'none',
  },
  // Pi: extension file (freshness + /astria command) in ~/.pi/agent/extensions,
  // AGENTS.md section, and the standard .mcp.json — the pi-mcp-adapter
  // extension reads it automatically, giving Pi the full MCP tool surface.
  pi: {
    skillFile: '',
    skillDst: '',
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'pi',
    mcp: 'pi',
  },
  // Amp reads AGENTS.md — the managed astria section is the whole install.
  amp: {
    skillFile: '',
    skillDst: '',
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'none',
  },
};

export const PLATFORM_NAMES = Object.keys(PLATFORMS);
