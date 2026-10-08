import { join } from 'path';
import { existsSync } from 'fs';

// The generated declarations in ../astria.node.d.ts are the single source
// of truth for the native surface: the lazy binding is typed as the module
// itself, so argument order, optionality, and result shapes are checked at
// compile time on both ends of every wrapper below. A native signature
// change without a d.ts update fails `tsc` instead of surfacing at runtime.
type NativeModule = typeof import('../astria.node');
type NativeFn = {
  [K in keyof NativeModule]: NativeModule[K] extends (...args: any[]) => any ? K : never;
}[keyof NativeModule];

const PLATFORM_SUFFIX: Record<string, string> = {
  'win32-x64': 'win32-x64-msvc',
  'win32-arm64': 'win32-arm64-msvc',
  'darwin-x64': 'darwin-x64',
  'darwin-arm64': 'darwin-arm64',
  'linux-x64': 'linux-x64-gnu',
  'linux-arm64': 'linux-arm64-gnu',
};

function isMusl(): boolean {
  const report = process.report!.getReport() as { header: { glibcVersionRuntime?: string } };
  return !report.header.glibcVersionRuntime;
}

export function getPlatformSuffix(): string {
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
    } catch (error: any) {
      // Missing packages and installed-but-unloadable binaries need different repairs.
      if (error.code === 'MODULE_NOT_FOUND' && error.message.includes(`'${name}'`)) return undefined;
      throw new Error(`Native package ${name} is installed but cannot load (${error.code ?? 'load error'}): ${error.message}`);
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
    case 'linux-x64-musl':
      return attempt('@nodesify/astria-linux-x64-musl');
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

function loadNativeBinding(): NativeModule {
  const suffix = getPlatformSuffix();
  const supported = ['win32-x64-msvc', 'win32-arm64-msvc', 'darwin-x64', 'darwin-arm64', 'linux-x64-gnu', 'linux-arm64-gnu', 'linux-x64-musl'];
  if (!supported.includes(suffix)) throw new Error(`Unsupported native platform: ${suffix}. Supported targets: ${supported.join(', ')}.`);
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
    `Note: Intel macOS, musl and windows-arm64 builds ship without the local embedding ` +
    `runtime (no prebuilt ONNX there) — everything else works.`
  );
}

// The binding loads lazily on first native call, not at import time: pure-JS
// commands (`astria install`, `astria uninstall`, `--version`) must work on a
// machine where the binary is missing — uninstall is the repair path.
let cachedBinding: NativeModule | undefined;
function binding(): NativeModule {
  if (cachedBinding === undefined) {
    cachedBinding = loadNativeBinding();
  }
  return cachedBinding;
}

// Every wrapper defers to binding() at call time (a missing binary throws
// one clear diagnostic when a native command actually runs, never at module
// load) while keeping the declared signature: the cast is checked against
// the generated declaration, so a drift between wrapper and native surface
// is a compile error here, not a runtime surprise.
function fn<K extends NativeFn>(name: K): NativeModule[K] {
  return ((...args: unknown[]) => {
    const call = (binding() as Record<string, (...a: unknown[]) => unknown>)[name];
    if (typeof call !== 'function') throw new Error(`Native binary does not provide ${name}; CLI and native versions do not match. Reinstall the matching platform package or rebuild the local native module, then restart MCP sessions.`);
    return call(...args);
  }) as NativeModule[K];
}

export const runPipeline = fn('runPipeline');
export const updatePipeline = fn('updatePipeline');
export const graphStats = fn('graphStats');
export const graphBuildInfo = fn('graphBuildInfo');
export const graphFreshness = fn('graphFreshness');
export const embeddingsSupported = fn('embeddingsSupported');
export const verifySourceCommit = fn('verifySourceCommit');
export const godNodes = fn('godNodes');
export const listCommunities = fn('listCommunities');
export const explainNode = fn('explainNode');
export const exportJsonCmd = fn('exportJsonCmd');
export const exportHtmlCmd = fn('exportHtmlCmd');
export const exportGraphmlCmd = fn('exportGraphmlCmd');
export const exportCypherCmd = fn('exportCypherCmd');
export const exportSvgCmd = fn('exportSvgCmd');
export const neo4jPushCmd = fn('neo4jPushCmd');
export const healthReport = fn('healthReport');
export const riskReport = fn('riskReport');
export const tokenBenchmark = fn('tokenBenchmark');
export const queryGraph = fn('queryGraph');
export const callflowMermaid = fn('callflowMermaid');
export const repoMap = fn('repoMap');
export const findPath = fn('findPath');
export const clusterOnly = fn('clusterOnly');
export const mergeGraphs = fn('mergeGraphs');
export const diffGraphs = fn('diffGraphs');
export const graphHistory = fn('graphHistory');
export const affectedNode = fn('affectedNode');
export const runMcpServer = fn('runMcpServer');
export const runMcpHttpServer = fn('runMcpHttpServer');
export const exportTree = fn('exportTree');
export const exportWiki = fn('exportWiki');
export const exportObsidian = fn('exportObsidian');
export const ingestUrl = fn('ingestUrl');
export const saveTranscript = fn('saveTranscript');
export const diagnoseGraph = fn('diagnoseGraph');
export const saveQueryResult = fn('saveQueryResult');
export const reflectGraph = fn('reflect');
export const globalAdd = fn('globalAdd');
export const globalRemove = fn('globalRemove');
export const globalList = fn('globalList');
export const globalPath = fn('globalPath');
export const ingestScip = fn('ingestScip');
export const ingestPostgres = fn('ingestPostgres');
