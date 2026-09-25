import { join } from 'path';
import { existsSync } from 'fs';

const PLATFORM_SUFFIX: Record<string, string> = {
  'win32-x64': 'win32-x64-msvc',
  'darwin-x64': 'darwin-x64',
  'darwin-arm64': 'darwin-arm64',
  'linux-x64': 'linux-x64-gnu',
  'linux-arm64': 'linux-arm64-gnu',
};

function isMusl(): boolean {
  try {
    const { execFileSync } = require('child_process') as typeof import('child_process');
    // execFileSync never invokes a shell: ldd runs with a fixed arg vector.
    const out = execFileSync('ldd', ['--version'], { encoding: 'utf-8' });
    return out.includes('musl');
  } catch {
    return false;
  }
}

function getPlatformSuffix(): string {
  if (process.platform === 'linux' && isMusl()) {
    return `linux-${process.arch}-musl`;
  }
  return PLATFORM_SUFFIX[`${process.platform}-${process.arch}`] || `${process.platform}-${process.arch}`;
}

/// Require the fallback platform package for a suffix. Module names are
/// literal strings in the switch arms — nothing is constructed at runtime —
/// so require() only ever sees fixed, known package names.
function requirePlatformPackage(suffix: string): any {
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
function loadNativeBinding(): any {
  const local = join(__dirname, '..', 'graphify.node');
  if (existsSync(local)) return require('../graphify.node');

  // tsx runs tests from src/, where CI's built binary lands in dist/
  const localDist = join(__dirname, '..', 'dist', 'graphify.node');
  if (existsSync(localDist)) return require('../dist/graphify.node');

  const localSrc = join(__dirname, 'graphify.node');
  if (existsSync(localSrc)) return require('./graphify.node');

  const platformBinding = requirePlatformPackage(getPlatformSuffix());
  if (platformBinding) {
    return platformBinding;
  }

  throw new Error(
    `@nodesify/graphify: failed to load native module for ${process.platform}-${process.arch}.\n` +
    `Tried: local graphify.node and the platform fallback package\n` +
    `Ensure the correct platform package is installed.`,
  );
}

const binding = loadNativeBinding();

export const runPipeline = binding.runPipeline;
export const updatePipeline = binding.updatePipeline;
export const graphStats = binding.graphStats;
export const explainNode = binding.explainNode;
export const exportJsonCmd = binding.exportJsonCmd;
export const exportHtmlCmd = binding.exportHtmlCmd;
export const exportGraphmlCmd = binding.exportGraphmlCmd;
export const exportCypherCmd = binding.exportCypherCmd;
export const tokenBenchmark = binding.tokenBenchmark;
export const queryGraph = binding.queryGraph;
export const repoMap = binding.repoMap;
export const findPath = binding.findPath;
export const clusterOnly = binding.clusterOnly;
export const mergeGraphs = binding.mergeGraphs;
export const diffGraphs = binding.diffGraphs;
export const graphHistory = binding.graphHistory;
export const affectedNode = binding.affectedNode;
export const runMcpServer = binding.runMcpServer;
export const exportTree = binding.exportTree;
export const exportWiki = binding.exportWiki;
export const exportObsidian = binding.exportObsidian;
export const ingestUrl = binding.ingestUrl;
export const diagnoseGraph = binding.diagnoseGraph;
export const saveQueryResult = binding.saveQueryResult;
export const reflectGraph = binding.reflect;
export const globalAdd = binding.globalAdd;
export const globalRemove = binding.globalRemove;
export const globalList = binding.globalList;
export const globalPath = binding.globalPath;
export const ingestScip = binding.ingestScip;
export const ingestPostgres = binding.ingestPostgres;