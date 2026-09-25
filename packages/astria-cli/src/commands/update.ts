import * as pathMod from 'path';
import { existsSync } from 'fs';
import { updatePipeline, exportWiki, tokenBenchmark } from '../native';

export async function updateCommand(
  path: string,
  opts: { dedup?: boolean; backend?: string; model?: string; embed?: boolean },
) {
  if (opts.backend) process.env.GRAPHIFY_LLM_BACKEND = opts.backend;
  if (opts.model) process.env.GRAPHIFY_LLM_MODEL = opts.model;
  try {
    console.log(`Running incremental rebuild on: ${path}`);
    const result = updatePipeline(path, opts.dedup === false, opts.embed === true);
    console.log(`Nodes: ${result.nodesAdded}, Edges: ${result.edgesAdded}, Communities: ${result.communities}`);
    console.log(`Report updated at: ${pathMod.join(path, '.graphify', 'graph_report.md')}`);
    // A wiki created via `run --wiki` or `wiki` would otherwise drift stale
    // after incremental updates; regenerate it when it exists.
    const wikiDir = pathMod.join(path, '.graphify', 'wiki');
    if (existsSync(pathMod.join(wikiDir, 'index.md'))) {
      const articles = exportWiki(path, wikiDir, 25);
      console.log(`Wiki regenerated: ${articles} articles -> ${pathMod.join(wikiDir, 'index.md')}`);
    }
    const benchmark = tokenBenchmark(path);
    if (benchmark) console.log(benchmark);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
