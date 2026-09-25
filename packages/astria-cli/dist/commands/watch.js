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
exports.watchCommand = watchCommand;
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
const native_1 = require("../native");
const CODE_EXTENSIONS = new Set([
    '.py', '.js', '.jsx', '.mjs', '.ts', '.tsx',
    '.rs', '.go', '.java', '.c', '.h', '.cpp', '.cc', '.cxx', '.hpp',
]);
const SKIP_DIRS = new Set(['.graphify', 'node_modules', 'target', '.git', 'dist', '__pycache__']);
async function watchCommand(watchPath, opts) {
    const debounceMs = parseInt(opts.debounce || '3000', 10);
    const resolved = path.resolve(watchPath);
    if (!fs.existsSync(resolved) || !fs.statSync(resolved).isDirectory()) {
        console.error(`Error: "${resolved}" is not a valid directory`);
        process.exitCode = 1;
        return;
    }
    let changedFiles = new Set();
    let timer = null;
    let watcher;
    try {
        watcher = fs.watch(resolved, { recursive: true }, (_event, filename) => {
            if (!filename)
                return;
            const filePath = filename.replace(/\\/g, '/');
            const parts = filePath.split('/');
            if (parts.some((p) => SKIP_DIRS.has(p)))
                return;
            const ext = path.extname(filePath).toLowerCase();
            if (!CODE_EXTENSIONS.has(ext))
                return;
            changedFiles.add(filePath);
            if (timer)
                clearTimeout(timer);
            timer = setTimeout(() => {
                const batch = [...changedFiles];
                changedFiles.clear();
                console.log(`\n[nodesify-graphify] ${batch.length} file(s) changed, rebuilding...`);
                try {
                    const result = (0, native_1.runPipeline)(resolved);
                    console.log(`[nodesify-graphify] Rebuilt: ${result.nodesAdded} nodes, ${result.edgesAdded} edges, ${result.communities} communities`);
                }
                catch (e) {
                    console.error(`[nodesify-graphify] Rebuild failed:`, e.message || e);
                }
            }, debounceMs);
        });
    }
    catch (err) {
        console.error(`Error: Failed to watch "${resolved}": ${err.message || err}`);
        process.exitCode = 1;
        return;
    }
    console.log(`[nodesify-graphify] Watching ${resolved} (debounce: ${debounceMs}ms)`);
    console.log('[nodesify-graphify] Press Ctrl+C to stop');
    if (process.platform === 'win32') {
        const readline = require('readline');
        const rl = readline.createInterface({ input: process.stdin });
        rl.on('SIGINT', () => {
            console.log('\n[nodesify-graphify] Stopped.');
            watcher.close();
            rl.close();
            process.exit(0);
        });
    }
    else {
        process.on('SIGINT', () => {
            console.log('\n[nodesify-graphify] Stopped.');
            watcher.close();
            process.exit(0);
        });
        process.on('SIGTERM', () => {
            console.log('\n[nodesify-graphify] Stopped.');
            watcher.close();
            process.exit(0);
        });
    }
}
//# sourceMappingURL=watch.js.map