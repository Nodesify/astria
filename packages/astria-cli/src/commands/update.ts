import * as pathMod from 'path';
import { existsSync } from 'fs';
import { updatePipeline, exportWiki, tokenBenchmark, graphBuildInfo } from '../native';
import { printLlmSummary } from './llm-summary';
import { VERSION } from '../version';

export interface UpdateOptions {
  dedup?: boolean;
  backend?: string;
  judge?: string;
  model?: string;
  embed?: boolean;
  labelCommunities?: boolean;
  deep?: boolean;
  /** Suppress progress lines and the token benchmark (used by git hooks). */
  quiet?: boolean;
  /** Skip when the graph was published less than N minutes ago (used by git hooks). */
  ifStale?: number;
}

/// Age of the published graph in minutes, or null when there is nothing to
/// measure (missing database, or a graph built before provenance stamping).
/// Hook-driven rebuilds use this to skip runs that would change nothing.
function pipelineAgeMinutes(root: string): number | null {
  try {
    const info = graphBuildInfo(root);
    if (!info?.graphPublishedAt) return null;
    const published = Number(info.graphPublishedAt);
    if (!Number.isFinite(published) || published <= 0) return null;
    return Math.round((Date.now() / 1000 - published) / 60);
  } catch {
    return null;
  }
}

export async function updateCommand(path: string, opts: UpdateOptions = {}) {
  if (opts.judge && !opts.backend) {
    console.error('Error: --judge requires --backend (the judge wraps an engine; it cannot generate extractions)');
    process.exitCode = 1;
    return;
  }
  if (opts.backend) process.env.ASTRIA_LLM_BACKEND = opts.backend;
  if (opts.judge) process.env.ASTRIA_LLM_JUDGE = opts.judge;
  if (opts.model) process.env.ASTRIA_LLM_MODEL = opts.model;
  try {
    if (opts.ifStale && opts.ifStale > 0) {
      const age = pipelineAgeMinutes(path);
      if (age !== null && age < opts.ifStale) {
        if (!opts.quiet) console.log(`Graph is fresh (${age} min old), skipping incremental rebuild`);
        return;
      }
    }

    if (!opts.quiet) console.log(`Running incremental rebuild on: ${path}`);
    const result = updatePipeline(path, opts.dedup === false, opts.embed === true, opts.labelCommunities === true, opts.deep === true, VERSION);
    if (!opts.quiet) console.log(`Nodes: ${result.nodesAdded}, Edges: ${result.edgesAdded}, Communities: ${result.communities}`);
    if (!opts.quiet) printLlmSummary(result);
    if (!opts.quiet) console.log(`Report updated at: ${pathMod.join(path, '.astria', 'graph_report.md')}`);
    // A wiki created via `run --wiki` or `wiki` would otherwise drift stale
    // after incremental updates; regenerate it when it exists.
    const wikiDir = pathMod.join(path, '.astria', 'wiki');
    if (existsSync(pathMod.join(wikiDir, 'index.md'))) {
      const articles = exportWiki(path, wikiDir, 25);
      if (!opts.quiet) console.log(`Wiki regenerated: ${articles} articles -> ${pathMod.join(wikiDir, 'index.md')}`);
    }
    const benchmark = tokenBenchmark(path);
    if (benchmark && !opts.quiet) console.log(benchmark);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}