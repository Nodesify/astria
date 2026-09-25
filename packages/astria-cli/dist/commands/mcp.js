"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.mcpCommand = mcpCommand;
const native_1 = require("../native");
async function mcpCommand(opts) {
    try {
        // Blocks serving newline-delimited JSON-RPC on stdio until stdin closes
        (0, native_1.runMcpServer)(opts.graph);
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=mcp.js.map