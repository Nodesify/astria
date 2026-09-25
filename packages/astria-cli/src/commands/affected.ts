import { affectedNode } from '../native';

export async function affectedCommand(node: string, opts: {
  graph: string;
  depth: string;
  relation?: string;
}) {
  try {
    const depth = parseInt(opts.depth, 10) || 2;
    const result = affectedNode(opts.graph, node, depth, opts.relation);
    if (result.total === 0) {
      console.log(`Nothing references "${result.seedLabel}" — no blast radius.`);
      return;
    }
    console.log(`Blast radius of "${result.seedLabel}" (depth ≤ ${depth}): ${result.total} node(s)`);
    console.log();
    let lastDepth = 0;
    for (const hit of result.hits) {
      if (hit.depth !== lastDepth) {
        lastDepth = hit.depth;
        console.log(`  depth ${hit.depth}:`);
      }
      const via = hit.viaFile ? `  [${hit.viaFile}]` : '';
      console.log(`    ${hit.label} (${hit.relation})${via}`);
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
