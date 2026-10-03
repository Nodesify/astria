import { repoMap } from '../native';
import { DEFAULT_QUERY_BUDGET } from '../defaults';

export async function mapCommand(opts: {
  graph: string;
  budget: string;
  detail?: string;
  json?: boolean;
}) {
  try {
    const budget = parseInt(opts.budget || String(DEFAULT_QUERY_BUDGET), 10);
    const result = repoMap(opts.graph, budget, opts.detail);
    if (opts.json) {
      console.log(JSON.stringify({ filesShown: result.filesShown, text: result.text }, null, 2));
      return;
    }
    console.log(result.text);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
