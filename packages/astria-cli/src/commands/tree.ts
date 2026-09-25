import * as path from 'path';
import { exportTree } from '../native';

export async function treeCommand(opts: {
  graph: string;
  out: string;
  maxChildren: string;
}) {
  try {
    const maxChildren = parseInt(opts.maxChildren, 10) || 40;
    const count = exportTree(opts.graph, opts.out, maxChildren);
    const resolved = path.isAbsolute(opts.out)
      ? opts.out
      : path.join(opts.graph, opts.out);
    console.log(`Tree exported: ${count} symbols -> ${resolved}`);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
