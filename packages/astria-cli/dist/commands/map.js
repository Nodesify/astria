"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.mapCommand = mapCommand;
const native_1 = require("../native");
async function mapCommand(opts) {
    try {
        const budget = parseInt(opts.budget || '2000', 10);
        const result = (0, native_1.repoMap)(opts.graph, budget, opts.detail);
        console.log(result.text);
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=map.js.map