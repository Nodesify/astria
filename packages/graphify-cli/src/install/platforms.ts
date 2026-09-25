import * as path from 'path';
import * as os from 'os';

export interface PlatformConfig {
  skillFile: string;
  skillDst: string;
  claudeMd: boolean;
  agentsMd: boolean;
  geminiMd: boolean;
  settingsHook: 'claude' | 'codex' | 'gemini' | 'opencode' | 'none';
  /** Register the graphify MCP server in the agent's project-scoped config. */
  mcp?: boolean;
}

export const PLATFORMS: Record<string, PlatformConfig> = {
  claude: {
    skillFile: 'skill.md',
    skillDst: path.join('.claude', 'skills', 'graphify', 'SKILL.md'),
    claudeMd: true,
    agentsMd: false,
    geminiMd: false,
    settingsHook: 'claude',
  },
  codex: {
    skillFile: 'skill-codex.md',
    skillDst: path.join('.agents', 'skills', 'graphify', 'SKILL.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'codex',
  },
  gemini: {
    skillFile: 'skill-gemini.md',
    skillDst: os.platform() === 'win32'
      ? path.join('.agents', 'skills', 'graphify', 'SKILL.md')
      : path.join('.gemini', 'skills', 'graphify', 'SKILL.md'),
    claudeMd: false,
    agentsMd: false,
    geminiMd: true,
    settingsHook: 'gemini',
  },
  opencode: {
    skillFile: 'skill-opencode.md',
    skillDst: path.join('.config', 'opencode', 'skills', 'graphify', 'SKILL.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'opencode',
  },
  cursor: {
    skillFile: '',
    skillDst: '',
    claudeMd: false,
    agentsMd: false,
    geminiMd: false,
    settingsHook: 'none',
  },
  kiro: {
    skillFile: 'skill.md',
    skillDst: path.join('.kiro', 'skills', 'graphify', 'SKILL.md'),
    claudeMd: false,
    agentsMd: false,
    geminiMd: false,
    settingsHook: 'none',
  },
  aider: {
    skillFile: 'skill-aider.md',
    skillDst: path.join('.aider', 'skills', 'graphify', 'SKILL.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'none',
  },
  copilot: {
    skillFile: 'skill-copilot.md',
    skillDst: path.join('.github', 'skills', 'graphify', 'SKILL.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'none',
  },
  trae: {
    skillFile: 'skill-trae.md',
    skillDst: path.join('.trae', 'skills', 'graphify', 'SKILL.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'none',
  },
  zcode: {
    // skill-codex.md is CLI-oriented, which is what ZCode sessions use
    // unless the graphify MCP server is registered by the mcp flag below.
    skillFile: 'skill-codex.md',
    skillDst: path.join('.zcode', 'skills', 'graphify', 'SKILL.md'),
    claudeMd: false,
    agentsMd: true,
    geminiMd: false,
    settingsHook: 'none',
    mcp: true,
  },
};

export const PLATFORM_NAMES = Object.keys(PLATFORMS);
