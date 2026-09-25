import { queryGraph } from '../native';

export async function queryCommand(question: string, opts: {
  graph: string;
  dfs: boolean;
  depth: string;
  budget: string;
  directed?: boolean;
  detail?: string;
  cursor?: string;
}) {
  try {
    const mode = opts.dfs ? 'dfs' : 'bfs';
    const depth = parseInt(opts.depth || '2', 10);
    const budget = parseInt(opts.budget || '2000', 10);
    const cursor = parseInt(opts.cursor || '0', 10) || 0;
    const result = queryGraph(
      opts.graph,
      question,
      mode,
      depth,
      budget,
      opts.directed ?? false,
      opts.detail,
      cursor
    );
    console.log(result.text);
    if (result.graphBuiltAt) {
      console.log(`# graph built at ${result.graphBuiltAt}`);
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
