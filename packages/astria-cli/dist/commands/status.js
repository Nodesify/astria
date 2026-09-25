"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.statusCommand = statusCommand;
const fs_1 = require("fs");
const native_1 = require("../native");
const STALE_THRESHOLD = 30;
const VERY_STALE_THRESHOLD = 120;
async function statusCommand(opts) {
    const dbPath = `${opts.graph}/.graphify/db.sqlite`;
    const graphJsonPath = `${opts.graph}/.graphify/graph.json`;
    if (!(0, fs_1.existsSync)(dbPath)) {
        console.log('Status: no graph found');
        console.log('Run `nodesify-graphify run .` to build the graph');
        return;
    }
    let stats;
    try {
        stats = (0, native_1.graphStats)(opts.graph);
    }
    catch (e) {
        console.log('Status: error reading graph database');
        console.log(e.message || String(e));
        process.exitCode = 1;
        return;
    }
    if (stats.nodeCount === 0) {
        console.log('Status: empty graph (0 nodes)');
        console.log('Run `nodesify-graphify run .` to populate the graph');
        return;
    }
    if (!(0, fs_1.existsSync)(graphJsonPath)) {
        console.log(`Status: incomplete (db has ${stats.nodeCount} nodes but no graph.json)`);
        console.log('Run `nodesify-graphify run .` to complete the build');
        return;
    }
    const mtime = (0, fs_1.statSync)(graphJsonPath).mtimeMs;
    const ageMinutes = Math.round((Date.now() - mtime) / 60000);
    let staleness;
    if (ageMinutes <= STALE_THRESHOLD) {
        staleness = 'fresh';
    }
    else if (ageMinutes <= VERY_STALE_THRESHOLD) {
        staleness = 'stale';
    }
    else {
        staleness = 'very_stale';
    }
    console.log(`Status: ${staleness} (${ageMinutes} min ago)`);
    console.log(`Nodes: ${stats.nodeCount}`);
    console.log(`Edges: ${stats.edgeCount}`);
    console.log(`Communities: ${stats.communityCount}`);
    console.log(`Files tracked: ${stats.fileCount}`);
    if (staleness === 'stale' || staleness === 'very_stale') {
        console.log(`Recommendation: run \`nodesify-graphify update .\` to refresh`);
    }
}
//# sourceMappingURL=status.js.map