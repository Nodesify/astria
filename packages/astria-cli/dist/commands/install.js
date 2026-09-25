"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.registerInstallCommand = registerInstallCommand;
const index_1 = require("../install/index");
const platforms_1 = require("../install/platforms");
function registerInstallCommand(program) {
    program
        .command('install')
        .description('Install nodesify-graphify skill for an AI platform')
        .option('--platform <name>', `Platform: ${platforms_1.PLATFORM_NAMES.join(', ')}`, 'claude')
        .action(async (opts) => {
        try {
            const results = (0, index_1.installPlatform)(opts.platform, process.cwd());
            for (const msg of results) {
                console.log(msg);
            }
        }
        catch (err) {
            console.error(err.message || err);
            process.exitCode = 1;
        }
    });
    program
        .command('uninstall')
        .description('Uninstall nodesify-graphify skill for an AI platform')
        .option('--platform <name>', `Platform: ${platforms_1.PLATFORM_NAMES.join(', ')}`, 'claude')
        .action(async (opts) => {
        try {
            const results = (0, index_1.uninstallPlatform)(opts.platform, process.cwd());
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
//# sourceMappingURL=install.js.map