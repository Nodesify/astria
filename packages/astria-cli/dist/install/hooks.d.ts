declare const LEGACY_HOOK_PREFIXES: ({
    js: string;
    shell?: undefined;
} | {
    shell: string;
    js?: undefined;
})[];
export declare function installGitHooks(projectDir: string): string[];
export declare function uninstallGitHooks(projectDir: string): string[];
export declare function statusGitHooks(projectDir: string): string[];
export { LEGACY_HOOK_PREFIXES };
//# sourceMappingURL=hooks.d.ts.map