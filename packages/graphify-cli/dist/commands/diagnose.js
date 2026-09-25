"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.diagnoseCommand = diagnoseCommand;
const native_1 = require("../native");
async function diagnoseCommand(opts) {
    try {
        const report = (0, native_1.diagnoseGraph)(opts.graph);
        if (opts.json) {
            const { text: _text, ...rest } = report;
            console.log(JSON.stringify(rest, null, 2));
            return;
        }
        console.log(report.text);
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=diagnose.js.map