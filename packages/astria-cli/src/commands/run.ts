import * as pathMod from 'path';
import { runPipeline, exportWiki, globalAdd } from '../native';
import { printLlmSummary } from './llm-summary';
import { VERSION } from '../version';
import { indexingOptions } from './indexing-profile';

export async function runCommand(
  path: string,
  opts: { dedup?: boolean; backend?: string; judge?: string; model?: string; wiki?: boolean; embed?: boolean; labelCommunities?: boolean; deep?: boolean; global?: boolean; as?: string },
) {
  if (opts.judge && !opts.backend) {
    console.error('Error: --judge requires --backend (the judge wraps an engine; it cannot generate extractions)');
    process.exitCode = 1;
    return;
  }
  try {
    opts = indexingOptions(path, opts, false);
    console.log(`Running astria pipeline on: ${path}`);
    const result = runPipeline(path, opts.dedup === false, opts.embed === true, opts.labelCommunities === true, opts.deep === true, VERSION);
    console.log(`Nodes added: ${result.nodesAdded}`);
    console.log(`Edges added: ${result.edgesAdded}`);
    console.log(`Communities: ${result.communities}`);
    printLlmSummary(result);
    console.log(`Report written to: ${pathMod.join(path, '.astria', 'graph_report.md')}`);
    if (opts.wiki) {
      const outDir = pathMod.join(path, '.astria', 'wiki');
      const articles = exportWiki(path, outDir, 25);
      console.log(`Wiki written: ${articles} articles -> ${pathMod.join(outDir, 'index.md')}`);
    }
    if (opts.global) {
      const merged = globalAdd(path, opts.as);
      console.log(`Global graph: repo '${merged.tag}' merged (${merged.nodesAdded} nodes, ${merged.edgesAdded} edges, ${merged.sameTypeEdges} same_type_as, ${merged.crossRepoCallEdges} cross-repo calls)`);
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
