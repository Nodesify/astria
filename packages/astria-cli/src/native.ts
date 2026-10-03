import { join } from 'path';
import { existsSync } from 'fs';

const PLATFORM_SUFFIX: Record<string, string> = {
  'win32-x64': 'win32-x64-msvc',
  'win32-arm64': 'win32-arm64-msvc',
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
/// so require() only ever sees fixed, known package names. Every arm is
/// guarded: when optionalDependencies did not install the package (npm
/// --omit=optional, a registry hiccup, pnpm/yarn quirks) the require throws
/// MODULE_NOT_FOUND, which surfaces as undefined here instead of crashing
/// the process with a raw stack — letting loadNativeBinding's diagnostic
/// below actually run for its target scenario.
function requirePlatformPackage(suffix: string): any {
  const attempt = (name: string): any => {
    try {
      return require(name);
    } catch {
      return undefined;
    }
  };
  switch (suffix) {
    case 'win32-x64-msvc':
      return attempt('@nodesify/astria-win32-x64-msvc');
    case 'win32-arm64-msvc':
      return attempt('@nodesify/astria-win32-arm64-msvc');
    case 'darwin-x64':
      return attempt('@nodesify/astria-darwin-x64');
    case 'darwin-arm64':
      return attempt('@nodesify/astria-darwin-arm64');
    case 'linux-x64-gnu':
      return attempt('@nodesify/astria-linux-x64-gnu');
    case 'linux-arm64-gnu':
      return attempt('@nodesify/astria-linux-arm64-gnu');
    // Musl targets are recognized so the error below can say exactly what
    // is missing; no musl platform package is published yet.
    case 'linux-x64-musl':
      return attempt('@nodesify/astria-linux-x64-musl');
    case 'linux-arm64-musl':
      return attempt('@nodesify/astria-linux-arm64-musl');
    default:
      return undefined;
  }
}

/// Local candidates are fixed paths relative to this module; each require()
/// below uses a literal relative specifier, guarded by existsSync so a
/// missing binary never throws at load time. The package-root copy wins, so
/// warn when a newer dist/ build exists — a stale root binary otherwise
/// silently shadows a fresh rebuild.
function warnIfShadowed(root: string, dist: string): void {
  try {
    const { statSync } = require('fs') as typeof import('fs');
    if (statSync(root).mtimeMs < statSync(dist).mtimeMs - 1000) {
      console.warn(
        `@nodesify/astria: ${root} is older than ${dist}; loading the stale binary.\n` +
        `Remove the package-root copy or rerun \`npm run napi:build\` so the fresh build is picked up.`,
      );
    }
  } catch {
    // Stat failures must never block loading.
  }
}

function loadNativeBinding(): any {
  const local = join(__dirname, '..', 'astria.node');
  if (existsSync(local)) {
    warnIfShadowed(local, join(__dirname, '..', 'dist', 'astria.node'));
    return require('../astria.node');
  }

  // tsx runs tests from src/, where CI's built binary lands in dist/
  const localDist = join(__dirname, '..', 'dist', 'astria.node');
  if (existsSync(localDist)) return require('../dist/astria.node');

  const localSrc = join(__dirname, 'astria.node');
  if (existsSync(localSrc)) return require('./astria.node');

  const platformBinding = requirePlatformPackage(getPlatformSuffix());
  if (platformBinding) {
    return platformBinding;
  }

  throw new Error(
    `@nodesify/astria: failed to load native module for ${process.platform}-${process.arch} ` +
    `(resolved target: ${getPlatformSuffix()}).\n` +
    `Tried: local astria.node and the platform fallback package.\n` +
    `If the platform package is missing, reinstall without --omit=optional ` +
    `(npm install @nodesify/astria --force). ` +
    `Note: musl and windows-arm64 builds ship without the local embedding ` +
    `runtime (no prebuilt ONNX there) — everything else works.`
  );
}

// The binding loads lazily on first native call, not at import time: pure-JS
// commands (`astria install`, `astria uninstall`, `--version`) must work on a
// machine where the binary is missing — uninstall is the repair path.
let cachedBinding: any;
function binding(): any {
  if (cachedBinding === undefined) {
    cachedBinding = loadNativeBinding();
  }
  return cachedBinding;
}

// Every export defers to binding() at call time; a missing binary therefore
// throws one clear diagnostic when a native command actually runs, and never
// at module load.
export const runPipeline = (...args: any[]) => binding().runPipeline(...args);
export const updatePipeline = (...args: any[]) => binding().updatePipeline(...args);
export const graphStats = (...args: any[]) => binding().graphStats(...args);
export const graphBuildInfo = (...args: any[]) => binding().graphBuildInfo(...args);
export const godNodes = (...args: any[]) => binding().godNodes(...args);
export const listCommunities = (...args: any[]) => binding().listCommunities(...args);
export const explainNode = (...args: any[]) => binding().explainNode(...args);
export const exportJsonCmd = (...args: any[]) => binding().exportJsonCmd(...args);
export const exportHtmlCmd = (...args: any[]) => binding().exportHtmlCmd(...args);
export const exportGraphmlCmd = (...args: any[]) => binding().exportGraphmlCmd(...args);
export const exportCypherCmd = (...args: any[]) => binding().exportCypherCmd(...args);
export const exportSvgCmd = (...args: any[]) => binding().exportSvgCmd(...args);
export const neo4jPushCmd = (...args: any[]) => binding().neo4jPushCmd(...args);
export const healthReport = (...args: any[]) => binding().healthReport(...args);
export const riskReport = (...args: any[]) => binding().riskReport(...args);
export const tokenBenchmark = (...args: any[]) => binding().tokenBenchmark(...args);
export const queryGraph = (...args: any[]) => binding().queryGraph(...args);
export const callflowMermaid = (...args: any[]) => binding().callflowMermaid(...args);
export const repoMap = (...args: any[]) => binding().repoMap(...args);
export const findPath = (...args: any[]) => binding().findPath(...args);
export const clusterOnly = (...args: any[]) => binding().clusterOnly(...args);
export const mergeGraphs = (...args: any[]) => binding().mergeGraphs(...args);
export const diffGraphs = (...args: any[]) => binding().diffGraphs(...args);
export const graphHistory = (...args: any[]) => binding().graphHistory(...args);
export const affectedNode = (...args: any[]) => binding().affectedNode(...args);
export const runMcpServer = (...args: any[]) => binding().runMcpServer(...args);
export const exportTree = (...args: any[]) => binding().exportTree(...args);
export const exportWiki = (...args: any[]) => binding().exportWiki(...args);
export const exportObsidian = (...args: any[]) => binding().exportObsidian(...args);
export const ingestUrl = (...args: any[]) => binding().ingestUrl(...args);
export const saveTranscript = (...args: any[]) => binding().saveTranscript(...args);
export const diagnoseGraph = (...args: any[]) => binding().diagnoseGraph(...args);
export const saveQueryResult = (...args: any[]) => binding().saveQueryResult(...args);
export const reflectGraph = (...args: any[]) => binding().reflect(...args);
export const globalAdd = (...args: any[]) => binding().globalAdd(...args);
export const globalRemove = (...args: any[]) => binding().globalRemove(...args);
export const globalList = (...args: any[]) => binding().globalList(...args);
export const globalPath = (...args: any[]) => binding().globalPath(...args);
export const ingestScip = (...args: any[]) => binding().ingestScip(...args);
export const ingestPostgres = (...args: any[]) => binding().ingestPostgres(...args);