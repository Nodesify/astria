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
    const depth = parseInt(opts.depth || '2', 10);
    const budget = parseInt(opts.budget || String(DEFAULT_QUERY_BUDGET), 10);
    const cursor = parseInt(opts.cursor || '0', 10) || 0;
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
