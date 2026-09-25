"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.historyCommand = historyCommand;
const native_1 = require("../native");
async function historyCommand(opts) {
    try {
        const limit = parseInt(opts.limit || '20', 10);
        const entries = (0, native_1.graphHistory)(opts.graph, limit);
        if (entries.length === 0) {
            console.log('No query history found.');
            return;
        }
        console.log(`Recent queries (showing ${entries.length}):\n`);
        for (const entry of entries) {
            console.log(`[#${entry.id}] ${entry.question}`);
            if (entry.answer) {
                const preview = entry.answer.split('\n')[0];
                console.log(`  ${preview.substring(0, 100)}${preview.length > 100 ? '...' : ''}`);
            }
            console.log();
        }
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=history.js.map