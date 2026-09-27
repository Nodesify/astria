/* eslint-disable @typescript-eslint/no-explicit-any */

export interface PipelineResultJs {
  nodesAdded: number;
  edgesAdded: number;
  communities: number;
  report: string;
  semanticCached: number;
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
}

export interface NodeJs {
  id: string;
  label: string;
  fileType: string;
  sourceFile: string;
  sourceLine: number | null;
  docstring: string | null;
  community: number | null;
}

export interface QueryResultJs {
  text: string;
  nodeCount: number;
  edgeCount: number;
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
  relation: string;
  confidence: string;
}

export interface ExplainResultJs {
  id: string;
  label: string;
  sourceFile: string;
  community: number | null;
  neighborCount: number;
  neighbors: EdgeInfoJs[];
}

export interface DiffResultJs {
  nodesAdded: number;
  nodesRemoved: number;
  edgesAdded: number;
  edgesRemoved: number;
  addedNodeLabels: string[];
  removedNodeLabels: string[];
}

export interface HistoryEntryJs {
  id: number;
  question: string;
  answer: string | null;
  queriedAt: string;
}

export function runPipeline(root: string, noDedup?: boolean, embed?: boolean, labelCommunities?: boolean, deep?: boolean): PipelineResultJs;
export function updatePipeline(root: string, noDedup?: boolean, embed?: boolean, labelCommunities?: boolean, deep?: boolean): PipelineResultJs;
export function graphStats(root: string): GraphStatsJs;
export function getNode(root: string, nodeId: string): NodeJs | null;
export function getNeighbors(root: string, nodeId: string): NodeJs[];
export function exportJsonCmd(root: string, outPath: string): void;
export function exportHtmlCmd(root: string, outPath: string, mode?: string): void;
export function exportGraphmlCmd(root: string, outPath: string): void;
export function exportCypherCmd(root: string, outPath: string): number;
export function exportWiki(root: string, outDir: string, maxKeyNodes?: number): number;
export function exportSvgCmd(root: string, outPath: string): SvgCountsJs;
export function neo4jPushCmd(root: string, url: string, user?: string | null, pass?: string | null): Neo4jPushCountsJs;
export function healthReport(root: string): HealthReportJs;
export function riskReport(root: string, staged?: boolean | null): RiskReportJs;
export function exportObsidian(root: string, outDir: string): number;
export function exportTree(root: string, out: string, maxChildren?: number): number;
export function tokenBenchmark(root: string): string;
export function queryGraph(
  root: string,
  question: string,
  mode: string,
  depth: number,
  budget: number,
): QueryResultJs;
export function findPath(root: string, source: string, target: string): PathResultJs;
export function explainNode(root: string, nodeId: string): ExplainResultJs | null;
export function clusterOnly(root: string): PipelineResultJs;
export function mergeGraphs(rootA: string, rootB: string, outRoot: string): PipelineResultJs;
export function diffGraphs(rootA: string, rootB: string): DiffResultJs;
export function graphHistory(root: string, limit: number): HistoryEntryJs[];
