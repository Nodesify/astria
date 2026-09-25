"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.exportCommand = exportCommand;
const native_1 = require("../native");
async function exportCommand(opts) {
    try {
        const format = opts.format || 'json';
        if (format === 'html') {
            const outPath = opts.out.replace(/\.json$/, '.html');
            (0, native_1.exportHtmlCmd)(opts.graph, outPath, opts.mode || 'standard');
            console.log(`Exported HTML to: ${outPath}`);
        }
        else if (format === 'graphml') {
            const outPath = opts.out.replace(/\.json$/, '.graphml');
            (0, native_1.exportGraphmlCmd)(opts.graph, outPath);
            console.log(`Exported GraphML to: ${outPath}`);
        }
        else if (format === 'cypher') {
            const outPath = opts.out.replace(/\.json$/, '.cypher');
            const statements = (0, native_1.exportCypherCmd)(opts.graph, outPath);
            console.log(`Exported Cypher to: ${outPath} (${statements} MERGE statements — idempotent, safe to re-run)`);
        }
        else {
            (0, native_1.exportJsonCmd)(opts.graph, opts.out);
            console.log(`Exported JSON to: ${opts.out}`);
        }
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=export.js.map