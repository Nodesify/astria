import { repoMap } from '../native';

export async function mapCommand(opts: {
  graph: string;
  budget: string;
  detail?: string;
}) {
  try {
    const budget = parseInt(opts.budget || '2000', 10);
    const result = repoMap(opts.graph, budget, opts.detail);
    console.log(result.text);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
