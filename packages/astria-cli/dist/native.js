"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ingestPostgres = exports.ingestScip = exports.globalPath = exports.globalList = exports.globalRemove = exports.globalAdd = exports.reflectGraph = exports.saveQueryResult = exports.diagnoseGraph = exports.ingestUrl = exports.exportObsidian = exports.exportWiki = exports.exportTree = exports.runMcpServer = exports.affectedNode = exports.graphHistory = exports.diffGraphs = exports.mergeGraphs = exports.clusterOnly = exports.findPath = exports.repoMap = exports.queryGraph = exports.tokenBenchmark = exports.exportCypherCmd = exports.exportGraphmlCmd = exports.exportHtmlCmd = exports.exportJsonCmd = exports.explainNode = exports.graphStats = exports.updatePipeline = exports.runPipeline = void 0;
const path_1 = require("path");
const fs_1 = require("fs");
const PLATFORM_SUFFIX = {
    'win32-x64': 'win32-x64-msvc',
    'darwin-x64': 'darwin-x64',
    'darwin-arm64': 'darwin-arm64',
    'linux-x64': 'linux-x64-gnu',
    'linux-arm64': 'linux-arm64-gnu',
};
function isMusl() {
    try {
        const { execFileSync } = require('child_process');
        // execFileSync never invokes a shell: ldd runs with a fixed arg vector.
        const out = execFileSync('ldd', ['--version'], { encoding: 'utf-8' });
        return out.includes('musl');
    }
    catch {
        return false;
    }
}
function getPlatformSuffix() {
    if (process.platform === 'linux' && isMusl()) {
        return `linux-${process.arch}-musl`;
    }
    return PLATFORM_SUFFIX[`${process.platform}-${process.arch}`] || `${process.platform}-${process.arch}`;
}
/// Require the fallback platform package for a suffix. Module names are
/// literal strings in the switch arms — nothing is constructed at runtime —
/// so require() only ever sees fixed, known package names.
function requirePlatformPackage(suffix) {
    switch (suffix) {
        case 'win32-x64-msvc':
            return require('@nodesify/graphify-win32-x64-msvc');
        case 'darwin-x64':
            return require('@nodesify/graphify-darwin-x64');
        case 'darwin-arm64':
            return require('@nodesify/graphify-darwin-arm64');
        case 'linux-x64-gnu':
            return require('@nodesify/graphify-linux-x64-gnu');
        case 'linux-arm64-gnu':
            return require('@nodesify/graphify-linux-arm64-gnu');
        case 'linux-x64-musl':
            return require('@nodesify/graphify-linux-x64-musl');
        case 'linux-arm64-musl':
            return require('@nodesify/graphify-linux-arm64-musl');
        default:
            return undefined;
    }
}
/// Local candidates are fixed paths relative to this module; each require()
/// below uses a literal relative specifier, guarded by existsSync so a
/// missing binary never throws at load time.
function loadNativeBinding() {
    const local = (0, path_1.join)(__dirname, '..', 'graphify.node');
    if ((0, fs_1.existsSync)(local))
        return require('../graphify.node');
    // tsx runs tests from src/, where CI's built binary lands in dist/
    const localDist = (0, path_1.join)(__dirname, '..', 'dist', 'graphify.node');
    if ((0, fs_1.existsSync)(localDist))
        return require('../dist/graphify.node');
    const localSrc = (0, path_1.join)(__dirname, 'graphify.node');
    if ((0, fs_1.existsSync)(localSrc))
        return require('./graphify.node');
    const platformBinding = requirePlatformPackage(getPlatformSuffix());
    if (platformBinding) {
        return platformBinding;
    }
    throw new Error(`@nodesify/graphify: failed to load native module for ${process.platform}-${process.arch}.\n` +
        `Tried: local graphify.node and the platform fallback package\n` +
        `Ensure the correct platform package is installed.`);
}
const binding = loadNativeBinding();
exports.runPipeline = binding.runPipeline;
exports.updatePipeline = binding.updatePipeline;
exports.graphStats = binding.graphStats;
exports.explainNode = binding.explainNode;
exports.exportJsonCmd = binding.exportJsonCmd;
exports.exportHtmlCmd = binding.exportHtmlCmd;
exports.exportGraphmlCmd = binding.exportGraphmlCmd;
exports.exportCypherCmd = binding.exportCypherCmd;
exports.tokenBenchmark = binding.tokenBenchmark;
exports.queryGraph = binding.queryGraph;
exports.repoMap = binding.repoMap;
exports.findPath = binding.findPath;
exports.clusterOnly = binding.clusterOnly;
exports.mergeGraphs = binding.mergeGraphs;
exports.diffGraphs = binding.diffGraphs;
exports.graphHistory = binding.graphHistory;
exports.affectedNode = binding.affectedNode;
exports.runMcpServer = binding.runMcpServer;
exports.exportTree = binding.exportTree;
exports.exportWiki = binding.exportWiki;
exports.exportObsidian = binding.exportObsidian;
exports.ingestUrl = binding.ingestUrl;
exports.diagnoseGraph = binding.diagnoseGraph;
exports.saveQueryResult = binding.saveQueryResult;
exports.reflectGraph = binding.reflect;
exports.globalAdd = binding.globalAdd;
exports.globalRemove = binding.globalRemove;
exports.globalList = binding.globalList;
exports.globalPath = binding.globalPath;
exports.ingestScip = binding.ingestScip;
exports.ingestPostgres = binding.ingestPostgres;
//# sourceMappingURL=native.js.map