"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.globalAddCommand = globalAddCommand;
exports.globalRemoveCommand = globalRemoveCommand;
exports.globalListCommand = globalListCommand;
exports.globalPathCommand = globalPathCommand;
const native_1 = require("../native");
async function globalAddCommand(path, opts) {
    try {
        const result = (0, native_1.globalAdd)(path, opts.as);
        console.log(`Repo '${result.tag}' merged into the global graph.`);
        console.log(`Nodes: ${result.nodesAdded} | Edges: ${result.edgesAdded} | ` +
            `same_type_as: ${result.sameTypeEdges} | cross-repo calls: ${result.crossRepoCallEdges}`);
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
async function globalRemoveCommand(tag) {
    try {
        const removed = (0, native_1.globalRemove)(tag);
        console.log(`Removed '${tag}' (${removed} nodes) from the global graph.`);
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
async function globalListCommand() {
    try {
        const entries = (0, native_1.globalList)();
        if (entries.length === 0) {
            console.log('Global graph is empty. Add repos with: astria global add <path>');
            return;
        }
        console.log('Global graph repos:');
        for (const e of entries) {
            console.log(`  ${e.tag} — ${e.nodes} nodes, ${e.edges} edges`);
        }
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
async function globalPathCommand(source, target) {
    try {
        const result = (0, native_1.globalPath)(source, target);
        if (!result) {
            console.log(`No path found between '${source}' and '${target}'.`);
            return;
        }
        console.log(result.replace(/ --(?=[^\s])/g, ' --\n  '));
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=global.js.map