import * as path from 'path';
import { exportWiki, exportObsidian } from '../native';

export async function wikiCommand(opts: {
  graph: string;
  out: string;
  maxNodes: string;
  format: string;
}) {
  try {
    const resolved = path.isAbsolute(opts.out)
      ? opts.out
      : path.join(opts.graph, opts.out);

    if (opts.format === 'obsidian') {
      const notes = exportObsidian(opts.graph, opts.out);
      console.log(`Obsidian vault exported: ${notes} notes -> ${resolved}`);
      console.log(`Open ${resolved} as a vault in Obsidian (graphify.canvas included)`);
      return;
    }

    const maxNodes = parseInt(opts.maxNodes, 10) || 25;
    const count = exportWiki(opts.graph, opts.out, maxNodes);
    console.log(`Wiki exported: ${count} articles -> ${resolved}`);
    console.log(`Start at: ${path.join(resolved, 'index.md')}`);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
