"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.diffCommand = diffCommand;
const native_1 = require("../native");
async function diffCommand(pathA, pathB) {
    try {
        const result = (0, native_1.diffGraphs)(pathA, pathB);
        console.log(`Nodes added: ${result.nodesAdded}`);
        console.log(`Nodes removed: ${result.nodesRemoved}`);
        console.log(`Edges added: ${result.edgesAdded}`);
        console.log(`Edges removed: ${result.edgesRemoved}`);
        if (result.addedNodeLabels.length > 0) {
            console.log('\nAdded nodes:');
            for (const label of result.addedNodeLabels.slice(0, 20)) {
                console.log(`  + ${label}`);
            }
            if (result.addedNodeLabels.length > 20) {
                console.log(`  ... and ${result.addedNodeLabels.length - 20} more`);
            }
        }
        if (result.removedNodeLabels.length > 0) {
            console.log('\nRemoved nodes:');
            for (const label of result.removedNodeLabels.slice(0, 20)) {
                console.log(`  - ${label}`);
            }
            if (result.removedNodeLabels.length > 20) {
                console.log(`  ... and ${result.removedNodeLabels.length - 20} more`);
            }
        }
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=diff.js.map