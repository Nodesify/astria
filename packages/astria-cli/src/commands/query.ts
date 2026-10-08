import { queryGraph } from '../native';
import { DEFAULT_QUERY_BUDGET } from '../defaults';

export async function queryCommand(question: string, opts: {
  graph: string;
  dfs: boolean;
  depth: string;
  budget: string;
  directed?: boolean;
  detail?: string;
  cursor?: string;
  noEmbed?: boolean;
  json?: boolean;
}) {
  try {
    const mode = opts.dfs ? 'dfs' : 'bfs';
    const depth = Number(opts.depth ?? '2');
    const budget = Number(opts.budget ?? String(DEFAULT_QUERY_BUDGET));
    const cursor = Number(opts.cursor ?? '0');
    if (![depth, budget, cursor].every(Number.isSafeInteger) || depth < 0 || cursor < 0) {
      throw new Error('depth, budget and cursor must be integers; depth and cursor must be nonnegative');
    }
    // The native layer reads ASTRIA_EMBED at query time; the flag shares the
    // same switch so CLI and MCP callers get identical behavior.
    if (opts.noEmbed) process.env.ASTRIA_EMBED = 'off';
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
    if (opts.json) {
      console.log(
        JSON.stringify(
          {
            question,
            mode,
            depth,
            budget,
            directed: opts.directed ?? false,
            nodeCount: result.nodeCount,
            edgeCount: result.edgeCount,
            // Present when the node list was truncated; pass back as --cursor.
            nextCursor: result.nextCursor ?? null,
            graphBuiltAt: result.graphBuiltAt ?? null,
            graphGeneration: result.graphGeneration ?? null,
            freshness: result.freshness ?? null,
            nodes: result.nodes,
            edges: result.edges,
            renderedTokens: result.renderedTokens,
            elapsedMilliseconds: result.elapsedMilliseconds,
            snapshotEstimatedBytes: result.snapshotEstimatedBytes,
            text: result.text,
          },
          null,
          2,
        ),
      );
      return;
    }
    process.stdout.write(result.text);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
