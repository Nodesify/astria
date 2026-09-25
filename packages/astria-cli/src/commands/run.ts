import * as pathMod from 'path';
import { runPipeline, exportWiki, tokenBenchmark, globalAdd } from '../native';

export async function runCommand(
  path: string,
  opts: { dedup?: boolean; backend?: string; model?: string; wiki?: boolean; embed?: boolean; global?: boolean; as?: string },
) {
  if (opts.backend) process.env.GRAPHIFY_LLM_BACKEND = opts.backend;
  if (opts.model) process.env.GRAPHIFY_LLM_MODEL = opts.model;
  try {
    console.log(`Running graphify pipeline on: ${path}`);
    const result = runPipeline(path, opts.dedup === false, opts.embed === true);
    console.log(`Nodes added: ${result.nodesAdded}`);
    console.log(`Edges added: ${result.edgesAdded}`);
    console.log(`Communities: ${result.communities}`);
    console.log(`Report written to: ${pathMod.join(path, '.graphify', 'graph_report.md')}`);
    if (opts.wiki) {
      const outDir = pathMod.join(path, '.graphify', 'wiki');
      const articles = exportWiki(path, outDir, 25);
      console.log(`Wiki written: ${articles} articles -> ${pathMod.join(outDir, 'index.md')}`);
    if (opts.global) {
      const merged = globalAdd(path, opts.as);
      console.log(`Global graph: repo '${merged.tag}' merged (${merged.nodesAdded} nodes, ${merged.edgesAdded} edges, ${merged.sameTypeEdges} same_type_as, ${merged.crossRepoCallEdges} cross-repo calls)`);
    }
    }
    const benchmark = tokenBenchmark(path);
    if (benchmark) console.log(benchmark);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
