import { explainNode } from '../native';

export async function explainCommand(node: string, opts: { graph: string; json?: boolean }) {
  try {
    const result = explainNode(opts.graph, node);
    if (!result) {
      console.log(`Node "${node}" not found`);
      return;
    }
    if (opts.json) {
      console.log(JSON.stringify(result, null, 2));
      return;
    }
    console.log(`Node: ${result.label}`);
    console.log(`  ID: ${result.id}`);
    // An unresolved global name (a `stub` node) has no owning file. Printing
    // whatever file referenced it first would name a file that does not
    // define the symbol; say so instead.
    if (result.sourceFile) {
      console.log(
        `  File: ${result.sourceFile}${result.sourceLine != null ? `:${result.sourceLine}` : ''}`
      );
    } else {
      console.log('  File: (no source locus — unresolved name, no single owner)');
    }
    if (result.community !== null && result.community !== undefined) {
      console.log(`  Community: ${result.community}`);
    }
    if (result.hyperedges && result.hyperedges.length > 0) {
      console.log(`  Hyperedges: ${result.hyperedges.join(', ')}`);
    }

    if (result.neighbors.length > 0) {
      console.log(`\nConnections (${result.neighborCount}):`);
      for (const n of result.neighbors) {
        const loc = n.neighborFile
          ? n.neighborLine != null
            ? ` (${n.neighborFile}:${n.neighborLine})`
            : ` (${n.neighborFile})`
          : '';
        // Direction is the stored edge's: `-->` this node calls/imports the
        // neighbor; `<--` the neighbor calls/imports this node.
        const arrow = n.outgoing === false ? '<--' : '-->';
        console.log(`  ${arrow} ${n.neighborLabel} [${n.relation}] [${n.confidence}]${loc}`);
      }
      if (result.neighborCount > result.neighbors.length) {
        const remaining = result.neighborCount - result.neighbors.length;
        console.log(`  ... and ${remaining} more`);
      }
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
