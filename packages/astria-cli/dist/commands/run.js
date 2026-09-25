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
exports.runCommand = runCommand;
const pathMod = __importStar(require("path"));
const native_1 = require("../native");
async function runCommand(path, opts) {
    if (opts.backend)
        process.env.GRAPHIFY_LLM_BACKEND = opts.backend;
    if (opts.model)
        process.env.GRAPHIFY_LLM_MODEL = opts.model;
    try {
        console.log(`Running graphify pipeline on: ${path}`);
        const result = (0, native_1.runPipeline)(path, opts.dedup === false, opts.embed === true);
        console.log(`Nodes added: ${result.nodesAdded}`);
        console.log(`Edges added: ${result.edgesAdded}`);
        console.log(`Communities: ${result.communities}`);
        console.log(`Report written to: ${pathMod.join(path, '.graphify', 'graph_report.md')}`);
        if (opts.wiki) {
            const outDir = pathMod.join(path, '.graphify', 'wiki');
            const articles = (0, native_1.exportWiki)(path, outDir, 25);
            console.log(`Wiki written: ${articles} articles -> ${pathMod.join(outDir, 'index.md')}`);
            if (opts.global) {
                const merged = (0, native_1.globalAdd)(path, opts.as);
                console.log(`Global graph: repo '${merged.tag}' merged (${merged.nodesAdded} nodes, ${merged.edgesAdded} edges, ${merged.sameTypeEdges} same_type_as, ${merged.crossRepoCallEdges} cross-repo calls)`);
            }
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
//# sourceMappingURL=run.js.map