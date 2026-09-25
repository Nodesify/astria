import * as path from 'path';
import { mergeGraphs } from '../native';

export async function mergeCommand(pathA: string, pathB: string, outPath: string) {
  try {
    console.log(`Merging graphs: ${pathA} + ${pathB} -> ${outPath}`);
    const result = mergeGraphs(pathA, pathB, outPath);
    console.log(`Nodes: ${result.nodesAdded}, Edges: ${result.edgesAdded}, Communities: ${result.communities}`);
    console.log(`Merged graph written to: ${path.join(outPath, '.graphify')}`);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
