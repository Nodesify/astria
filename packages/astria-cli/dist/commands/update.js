"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.updateCommand = updateCommand;
const pathMod = __importStar(require("path"));
const fs_1 = require("fs");
const native_1 = require("../native");
async function updateCommand(path, opts) {
    if (opts.backend)
        process.env.GRAPHIFY_LLM_BACKEND = opts.backend;
    if (opts.model)
        process.env.GRAPHIFY_LLM_MODEL = opts.model;
    try {
        console.log(`Running incremental rebuild on: ${path}`);
        const result = (0, native_1.updatePipeline)(path, opts.dedup === false, opts.embed === true);
        console.log(`Nodes: ${result.nodesAdded}, Edges: ${result.edgesAdded}, Communities: ${result.communities}`);
        console.log(`Report updated at: ${pathMod.join(path, '.graphify', 'graph_report.md')}`);
        // A wiki created via `run --wiki` or `wiki` would otherwise drift stale
        // after incremental updates; regenerate it when it exists.
        const wikiDir = pathMod.join(path, '.graphify', 'wiki');
        if ((0, fs_1.existsSync)(pathMod.join(wikiDir, 'index.md'))) {
            const articles = (0, native_1.exportWiki)(path, wikiDir, 25);
            console.log(`Wiki regenerated: ${articles} articles -> ${pathMod.join(wikiDir, 'index.md')}`);
        }
        const benchmark = (0, native_1.tokenBenchmark)(path);
        if (benchmark)
            console.log(benchmark);
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=update.js.map