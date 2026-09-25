"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.addCommand = addCommand;
const native_1 = require("../native");
async function addCommand(url, opts) {
    if (!url && !opts.scip && !opts.postgres) {
        console.error('Error: provide a URL, or --scip <file>, or --postgres <dsn>');
        process.exitCode = 1;
        return;
    }
    try {
        if (opts.scip) {
            const counts = (0, native_1.ingestScip)(opts.graph, opts.scip);
            console.log(`SCIP index ingested: ${counts.nodesAdded} nodes, ${counts.edgesAdded} edges`);
            return;
        }
        if (opts.postgres) {
            const counts = (0, native_1.ingestPostgres)(opts.graph, opts.postgres);
            console.log(`Postgres schema ingested: ${counts.nodesAdded} nodes, ${counts.edgesAdded} edges`);
            return;
        }
        console.log(`Fetching ${url}...`);
        const result = (0, native_1.ingestUrl)(opts.graph, url, opts.author, opts.contributor);
        console.log(`Saved: ${result.savedPath}`);
        if (result.graphUpdated) {
            console.log('Graph updated with the new content.');
        }
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=add.js.map