"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.pathCommand = pathCommand;
const native_1 = require("../native");
async function pathCommand(source, target, opts) {
    try {
        const result = (0, native_1.findPath)(opts.graph, source, target, opts.directed ?? false, opts.detail);
        if (!result.found) {
            console.log(result.text);
            return;
        }
        console.log(result.text);
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=path.js.map