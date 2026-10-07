import { riskReport } from '../native';

export interface ChangedDeclaration {
  id: string; label: string; file: string; line: number; endLine: number;
  kind: string; snapshot: 'before' | 'after'; change: string;
  owners: string[]; ownerSource: string | null;
}

export interface ChangeConsumer {
  id: string; label: string; file: string; line: number | null;
  snapshot: 'before' | 'after'; changedId: string; depth: number; evidence: string;
  stillPresent: boolean; isTest: boolean; owners: string[]; ownerSource: string | null;
  path: Array<{ from: string; to: string; relation: string; evidence: string; file: string; line: number | null }>;
}

export interface ChangeReview {
  schemaVersion: 1;
  score: number | null; level: string; scope: string;
  baseCommit: string; requestedBaseCommit: string; headCommit: string | null;
  beforeIdentity: string; afterIdentity: string;
  changedFiles: Array<{ status: string; oldPath: string | null; newPath: string | null }>;
  declarations: ChangedDeclaration[]; consumers: ChangeConsumer[];
  impacted: number; directConsumers: number; inferredConsumers: number; testConsumers: number;
  coverageComplete: boolean; coverageIssues: string[];
  unresolvedBefore: number; unresolvedAfter: number;
  filesIndexedBefore: number; filesIndexedAfter: number; indexingMs: number;
  text: string;
}

export interface ReviewOptions { graph: string; staged?: boolean; base?: string; head?: string }

/** One native engine serves local review, merge gates, and the PR dashboard. */
export function changeReview(opts: ReviewOptions): ChangeReview {
  if (opts.head && !opts.base) throw new Error('--head requires --base');
  if (opts.staged && opts.base) throw new Error('--staged cannot be combined with --base');
  const result = riskReport(opts.graph, opts.staged === true, opts.base, opts.base ? (opts.head ?? 'HEAD') : undefined);
  const report: ChangeReview = JSON.parse(result.reportJson);
  return { ...report, text: result.text };
}
