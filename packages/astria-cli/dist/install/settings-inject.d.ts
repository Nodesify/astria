export declare function injectClaudeHook(projectDir: string): boolean;
export declare function removeClaudeHook(projectDir: string): boolean;
export declare function injectCodexHook(projectDir: string): boolean;
export declare function removeCodexHook(projectDir: string): boolean;
export declare function injectGeminiHook(projectDir: string): boolean;
export declare function removeGeminiHook(projectDir: string): boolean;
export declare function injectOpenCodePlugin(projectDir: string): boolean;
export declare function removeOpenCodePlugin(projectDir: string): boolean;
export declare function injectCursorRule(projectDir: string): boolean;
export declare function removeCursorRule(projectDir: string): boolean;
export type McpFlavor = 'zcode' | 'claude' | 'cursor' | 'gemini';
export declare function injectAgentMcp(projectDir: string, flavor: McpFlavor): boolean;
export declare function removeAgentMcp(projectDir: string, flavor: McpFlavor): boolean;
export declare function injectZcodeMcp(projectDir: string): boolean;
export declare function removeZcodeMcp(projectDir: string): boolean;
export declare function injectKiroSteering(projectDir: string): boolean;
export declare function removeKiroSteering(projectDir: string): boolean;
//# sourceMappingURL=settings-inject.d.ts.map