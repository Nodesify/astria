"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.explainCommand = explainCommand;
const native_1 = require("../native");
async function explainCommand(node, opts) {
    try {
        const result = (0, native_1.explainNode)(opts.graph, node);
        if (!result) {
            console.log(`Node "${node}" not found`);
            return;
        }
        console.log(`Node: ${result.label}`);
        console.log(`  ID: ${result.id}`);
        console.log(`  File: ${result.sourceFile}${result.sourceLine != null ? `:${result.sourceLine}` : ''}`);
        if (result.community !== null && result.community !== undefined) {
            console.log(`  Community: ${result.community}`);
        }
        if (result.hyperedges && result.hyperedges.length > 0) {
            console.log(`  Hyperedges: ${result.hyperedges.join(', ')}`);
        }
        if (result.neighbors.length > 0) {
            console.log(`\nConnections (${result.neighborCount}):`);
            for (const n of result.neighbors) {
                const loc = n.neighborLine != null ? ` (${n.neighborFile}:${n.neighborLine})` : '';
                console.log(`  --> ${n.neighborLabel} [${n.relation}] [${n.confidence}]${loc}`);
            }
            if (result.neighborCount > result.neighbors.length) {
                const remaining = result.neighborCount - result.neighbors.length;
                console.log(`  ... and ${remaining} more`);
            }
        }
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=explain.js.map