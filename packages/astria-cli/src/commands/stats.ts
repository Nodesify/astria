import { graphStats } from '../native';

export async function statsCommand(opts: { graph: string; json?: boolean }) {
  try {
    const stats = graphStats(opts.graph);
    if (opts.json) {
      console.log(JSON.stringify(stats, null, 2));
      return;
    }
    console.log(`Nodes: ${stats.nodeCount}`);
    console.log(`Edges: ${stats.edgeCount}`);
    console.log(`Communities: ${stats.communityCount}`);
    console.log(`Files tracked: ${stats.fileCount}`);
    console.log(
      `Embeddings: ${stats.embeddingsSupported ? 'available' : 'not supported in this build'}`,
    );
    const types = Object.entries((stats.typeCounts ?? {}) as Record<string, number>)
      .sort((a, b) => b[1] - a[1])
      .map(([t, n]) => `${t}: ${n}`)
      .join(' | ');
    if (types) console.log(`Node types: ${types}`);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
