import * as path from 'path';
import { mergeGraphs } from '../native';

export interface MergeOptions {
  sameRepo?: boolean;
}

export async function mergeCommand(pathA: string, pathB: string, outPath: string, opts: MergeOptions = {}) {
  try {
    console.log(
      `Merging graphs: ${pathA} + ${pathB} -> ${outPath}` +
        (opts.sameRepo ? ' (same repository: shared ids, conflicts error)' : ' (cross-repository: ids namespaced per root)'),
    );
    const result = mergeGraphs(pathA, pathB, outPath, opts.sameRepo === true);
    console.log(`Nodes: ${result.nodesAdded}, Edges: ${result.edgesAdded}, Communities: ${result.communities}`);
    console.log(`Merged graph written to: ${path.join(outPath, '.astria')}`);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
