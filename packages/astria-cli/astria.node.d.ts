/* eslint-disable @typescript-eslint/no-explicit-any */

export interface PipelineResultJs {
  nodesAdded: number;
  edgesAdded: number;
  communities: number;
  report: string;
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

export function runPipeline(root: string, noDedup?: boolean, embed?: boolean): PipelineResultJs;
export function updatePipeline(root: string, noDedup?: boolean, embed?: boolean): PipelineResultJs;
export function graphStats(root: string): GraphStatsJs;
export function getNode(root: string, nodeId: string): NodeJs | null;
export function getNeighbors(root: string, nodeId: string): NodeJs[];
export function exportJsonCmd(root: string, outPath: string): void;
export function exportHtmlCmd(root: string, outPath: string, mode?: string): void;
export function exportGraphmlCmd(root: string, outPath: string): void;
export function exportCypherCmd(root: string, outPath: string): number;
export function exportWiki(root: string, outDir: string, maxKeyNodes?: number): number;
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
