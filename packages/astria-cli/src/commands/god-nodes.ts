import { godNodes } from '../native';

export async function godNodesCommand(opts: { graph: string; json?: boolean }) {
  try {
    const nodes = godNodes(opts.graph);
    if (opts.json) {
      console.log(JSON.stringify(nodes, null, 2));
      return;
    }
    if (nodes.length === 0) {
      console.log('No hub nodes found');
      return;
    }
    console.log('Top hubs:');
    for (const n of nodes) {
      console.log(`  ${n.label} (degree ${n.degree}, community ${n.community ?? '-'})`);
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
