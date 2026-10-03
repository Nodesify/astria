/* eslint-disable @typescript-eslint/no-explicit-any */

// Hand-maintained mirror of the #[napi] surface in crates/astria-napi/src/lib.rs.
// native.ts types the lazy binding as this module, so any drift between the
// Rust signatures and this file is a compile error in the CLI, not a runtime
// surprise. Field names are camelCase (napi's default rename).

export interface PipelineResultJs {
  nodesAdded: number;
  edgesAdded: number;
  communities: number;
  report: string;
  semanticCached: number;
  semanticGated: number;
  llmInputTokens: number;
  llmOutputTokens: number;
  llmApiCalls: number;
  communitiesLabeled: number;
  communitiesReused: number;
  communitiesFailed: number;
  deepLinks: number;
}

export interface HealthReportJs {
  score: number;
  grade: string;
  deadCode: Array<string>;
  cycles: Array<string>;
  hubs: Array<string>;
  ageDays: number | null;
  nodeCount: number;
  edgeCount: number;
  text: string;
}

export interface DiagnoseReportJs {
  nodeCount: number;
  edgeCount: number;
  danglingEdges: number;
  selfLoops: number;
  duplicateEdges: number;
  stubNodes: number;
  unlinkedNodes: number;
  fileTypeCounts: Array<string>;
  topDanglingTargets: Array<string>;
  text: string;
}

export interface RiskReportJs {
  score: number;
  level: string;
  changedFiles: Array<string>;
  filesWithSymbols: number;
  impacted: number;
  byDepth: Array<string>;
  communities: Array<string>;
  entries: Array<string>;
  text: string;
}

export interface SvgCountsJs {
  nodes: number;
  edges: number;
  communities: number;
  truncated: boolean;
}

export interface Neo4jPushCountsJs {
  nodes: number;
  edges: number;
  communities: number;
  statements: number;
}

export interface GraphStatsJs {
  nodeCount: number;
  edgeCount: number;
  communityCount: number;
  fileCount: number;
  typeCounts: Record<string, number>;
  /** Whether this build includes the local embedding runtime. */
  embeddingsSupported: boolean;
}

export interface GraphBuildInfoJs {
  graphPublishedAt: string | null;
  astriaVersion: string | null;
  pipelineVersion: string | null;
  extractionHashVersion: string | null;
  buildConfiguration: string | null;
  currentExtractionHashVersion: string;
}

export interface SourceCoverageJs {
  /** The project is a git work tree and git ran successfully. */
  insideGit: boolean;
  /** Why git could not be consulted, when it could not. */
  gitError: string | null;
  currentHead: string | null;
  /** HEAD recorded in the graph at publication time; null on graphs built
   *  before commit provenance was recorded. */
  recordedHead: string | null;
  /** Whether the work tree has uncommitted/untracked changes, when known. */
  treeDirty: boolean | null;
  filesChecked: number;
  filesMismatched: number;
  filesMissing: number;
  /** Up to five sample paths that drifted, for the gate's detail line. */
  driftSamples: string[];
  /** true/false = proven; null = cannot determine (not a repository, git
   *  failure, or unresolvable dirty-tree ambiguity). */
  coversHead: boolean | null;
  reason: string;
}

export interface GodNodeJs {
  id: string;
  label: string;
  degree: number;
  community: string | null;
}

export interface CommunityJs {
  id: number;
  label: string;
  summary: string | null;
  labelSource: string;
  cohesion: number | null;
  size: number;
}

export interface CommunitiesJs {
  modularity: number | null;
  communities: Array<CommunityJs>;
}

export interface QueryResultJs {
  text: string;
  nodeCount: number;
  edgeCount: number;
  /** Present when the node list was truncated — pass back as `cursor`. */
  nextCursor: number | null;
  graphBuiltAt: string | null;
}

export interface RepoMapJs {
  text: string;
  filesShown: number;
}

export interface PathResultJs {
  found: boolean;
  hops: number;
  text: string;
}

export interface EdgeInfoJs {
  neighborId: string;
  neighborLabel: string;
  neighborFile: string;
  neighborLine: number | null;
  /** True when the edge points from the explained node to the neighbor. */
  outgoing: boolean;
  relation: string;
  confidence: string;
  confidenceScore: number | null;
}

export interface ExplainResultJs {
  id: string;
  label: string;
  sourceFile: string;
  sourceLine: number | null;
  community: number | null;
  hyperedges: Array<string>;
  neighborCount: number;
  neighbors: Array<EdgeInfoJs>;
}

export interface AffectedHitJs {
  id: string;
  label: string;
  depth: number;
  relation: string;
  provenance: string;
  viaFile: string;
}

export interface AffectedResultJs {
  seed: string;
  seedLabel: string;
  total: number;
  hits: Array<AffectedHitJs>;
}

export interface DiffResultJs {
  nodesAdded: number;
  nodesRemoved: number;
  edgesAdded: number;
  edgesRemoved: number;
  addedNodeLabels: Array<string>;
  removedNodeLabels: Array<string>;
}

export interface HistoryEntryJs {
  id: number;
  question: string;
  answer: string | null;
  queriedAt: string;
}

export interface SavedResultJs {
  memoryPath: string;
  nodeId: string;
}

export interface GlobalAddResultJs {
  tag: string;
  nodesAdded: number;
  edgesAdded: number;
  sameTypeEdges: number;
  crossRepoCallEdges: number;
}

export interface GlobalListEntryJs {
  tag: string;
  nodes: number;
  edges: number;
}

export interface IngestCountsJs {
  nodesAdded: number;
  edgesAdded: number;
}

export interface IngestResultJs {
  savedPath: string;
  graphUpdated: boolean;
}

export interface SemanticCandidateJs {
  nodeId: string;
  cosine: number;
}

export interface McpHttpOptionsJs {
  root: string;
  host?: string | null;
  port?: number | null;
  token?: string | null;
  projects?: Array<string> | null;
  /** Browser origins allowed to send requests (exact match). */
  allowedOrigins?: Array<string> | null;
}

export function diagnoseGraph(root: string): DiagnoseReportJs;
export function riskReport(
  root: string,
  staged?: boolean | null,
  base?: string | null,
  head?: string | null,
): RiskReportJs;
export function exportSvgCmd(root: string, outPath: string): SvgCountsJs;
export function neo4jPushCmd(
  root: string,
  url: string,
  user?: string | null,
  pass?: string | null,
): Neo4jPushCountsJs;
export function healthReport(root: string): HealthReportJs;
export function saveQueryResult(
  root: string,
  question: string,
  answer: string,
  outcome?: string | null,
  correction?: string | null,
  sourceNodes?: Array<string> | null,
): SavedResultJs;
export function reflect(root: string): string;
export function globalAdd(root: string, tag?: string | null): GlobalAddResultJs;
export function globalRemove(tag: string): number;
export function globalList(): Array<GlobalListEntryJs>;
export function globalPath(source: string, target: string): string | null;
export function ingestScip(root: string, scipPath: string): IngestCountsJs;
export function ingestPostgres(root: string, dsn: string): IngestCountsJs;
export function runPipeline(
  root: string,
  noDedup?: boolean,
  embed?: boolean,
  labelCommunities?: boolean,
  deep?: boolean,
  cliVersion?: string | null,
): PipelineResultJs;
export function updatePipeline(
  root: string,
  noDedup?: boolean,
  embed?: boolean,
  labelCommunities?: boolean,
  deep?: boolean,
  cliVersion?: string | null,
): PipelineResultJs;
export function embeddingsSupported(): boolean;
export function graphStats(root: string): GraphStatsJs;
export function graphBuildInfo(root: string): GraphBuildInfoJs;
export function verifySourceCommit(root: string): SourceCoverageJs;
export function godNodes(root: string): Array<GodNodeJs>;
export function listCommunities(root: string): CommunitiesJs;
export function exportJsonCmd(root: string, outPath: string): void;
export function exportHtmlCmd(root: string, outPath: string, mode?: string | null): void;
export function exportGraphmlCmd(root: string, outPath: string): void;
export function exportCypherCmd(root: string, outPath: string): number;
export function tokenBenchmark(root: string): string;
export function callflowMermaid(
  root: string,
  node: string,
  depth: number,
  direction: string,
): string;
export function queryGraph(
  root: string,
  question: string,
  mode: string,
  depth: number,
  budget: number,
  directed?: boolean | null,
  detail?: string | null,
  cursor?: number | null,
): QueryResultJs;
export function repoMap(root: string, budget: number, detail?: string | null): RepoMapJs;
export function findPath(
  root: string,
  source: string,
  target: string,
  directed?: boolean | null,
  detail?: string | null,
): PathResultJs;
export function explainNode(root: string, nodeId: string): ExplainResultJs | null;
export function affectedNode(
  root: string,
  node: string,
  depth?: number | null,
  relation?: string | null,
): AffectedResultJs;
export function exportTree(root: string, out: string, maxChildren?: number | null): number;
export function exportWiki(root: string, outDir: string, maxKeyNodes?: number | null): number;
export function exportObsidian(root: string, outDir: string): number;
export function semanticCandidates(root: string, question: string): Array<SemanticCandidateJs>;
export function ingestUrl(
  root: string,
  url: string,
  author?: string | null,
  contributor?: string | null,
): IngestResultJs;
export function saveTranscript(
  root: string,
  source?: string | null,
  content?: string | null,
): IngestResultJs;
export function runMcpServer(root: string): void;
export function runMcpHttpServer(opts: McpHttpOptionsJs): void;
export function clusterOnly(
  root: string,
  resolution?: number | null,
  excludeHubs?: boolean | null,
): PipelineResultJs;
export function mergeGraphs(
  rootA: string,
  rootB: string,
  outRoot: string,
  sameRepo?: boolean | null,
): PipelineResultJs;
export function diffGraphs(rootA: string, rootB: string): DiffResultJs;
export function graphHistory(root: string, limit: number): Array<HistoryEntryJs>;
