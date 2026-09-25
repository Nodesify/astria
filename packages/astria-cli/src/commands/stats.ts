import { graphStats } from '../native';

export async function statsCommand(opts: { graph: string }) {
  try {
    const stats = graphStats(opts.graph);
    console.log(`Nodes: ${stats.nodeCount}`);
    console.log(`Edges: ${stats.edgeCount}`);
    console.log(`Communities: ${stats.communityCount}`);
    console.log(`Files tracked: ${stats.fileCount}`);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
