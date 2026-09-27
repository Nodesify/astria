import { explainNode } from '../native';

export async function neighborsCommand(
  node: string,
  opts: { graph: string; relation?: string; json?: boolean },
) {
  try {
    const result = explainNode(opts.graph, node);
    if (!result) {
      console.error(`Node not found: ${node}`);
      process.exitCode = 1;
      return;
    }
    const neighbors = opts.relation
      ? result.neighbors.filter((n: any) => n.relation === opts.relation)
      : result.neighbors;
    if (opts.json) {
      console.log(
        JSON.stringify(
          { node: result.label, id: result.id, file: result.sourceFile, neighbors },
          null,
          2,
        ),
      );
      return;
    }
    const filter = opts.relation ? ` via ${opts.relation}` : '';
    console.log(`Neighbors of ${result.label} (${neighbors.length}${filter}):`);
    if (neighbors.length === 0) {
      console.log('  (no matching neighbors)');
      return;
    }
    for (const n of neighbors) {
      const loc = n.neighborLine != null ? ` (${n.neighborFile}:${n.neighborLine})` : '';
      console.log(`  ${n.neighborLabel} [${n.relation}]${loc}`);
    }
    // The native layer caps the returned list; only the unfiltered view
    // hides entries.
    if (!opts.relation && result.neighborCount > result.neighbors.length) {
      const remaining = result.neighborCount - result.neighbors.length;
      console.log(`  ... and ${remaining} more`);
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
