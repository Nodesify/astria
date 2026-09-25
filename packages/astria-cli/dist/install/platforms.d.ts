export interface PlatformConfig {
    skillFile: string;
    skillDst: string;
    claudeMd: boolean;
    agentsMd: boolean;
    geminiMd: boolean;
    settingsHook: 'claude' | 'codex' | 'gemini' | 'opencode' | 'none';
    /** Register the astria MCP server in this platform's project-scoped config. */
    mcp?: 'zcode' | 'claude' | 'cursor' | 'gemini';
}
export declare const PLATFORMS: Record<string, PlatformConfig>;
export declare const PLATFORM_NAMES: string[];
//# sourceMappingURL=platforms.d.ts.map