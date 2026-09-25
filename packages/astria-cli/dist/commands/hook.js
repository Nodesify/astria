"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.registerHookCommand = registerHookCommand;
const hooks_1 = require("../install/hooks");
function registerHookCommand(program) {
    const hook = program.command('hook').description('Manage git hooks for auto-rebuild');
    hook
        .command('install')
        .description('Install post-commit and post-checkout hooks')
        .action(() => {
        try {
            const results = (0, hooks_1.installGitHooks)('.');
            for (const msg of results) {
                console.log(msg);
            }
        }
        catch (err) {
            console.error(err.message || err);
            process.exitCode = 1;
        }
    });
    hook
        .command('uninstall')
        .description('Remove nodesify-graphify git hooks')
        .action(() => {
        try {
            const results = (0, hooks_1.uninstallGitHooks)('.');
            for (const msg of results) {
                console.log(msg);
            }
        }
        catch (err) {
            console.error(err.message || err);
            process.exitCode = 1;
        }
    });
    hook
        .command('status')
        .description('Show git hook status')
        .action(() => {
        try {
            const results = (0, hooks_1.statusGitHooks)('.');
            for (const msg of results) {
                console.log(msg);
            }
        }
        catch (err) {
            console.error(err.message || err);
            process.exitCode = 1;
        }
    });
}
//# sourceMappingURL=hook.js.map