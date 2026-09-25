"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.statsCommand = statsCommand;
const native_1 = require("../native");
async function statsCommand(opts) {
    try {
        const stats = (0, native_1.graphStats)(opts.graph);
        console.log(`Nodes: ${stats.nodeCount}`);
        console.log(`Edges: ${stats.edgeCount}`);
        console.log(`Communities: ${stats.communityCount}`);
        console.log(`Files tracked: ${stats.fileCount}`);
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=stats.js.map