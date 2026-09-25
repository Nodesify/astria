import { ingestUrl, ingestScip, ingestPostgres } from '../native';

export async function addCommand(url: string, opts: {
  graph: string;
  author?: string;
  contributor?: string;
  scip?: string;
  postgres?: string;
}) {
  if (!url && !opts.scip && !opts.postgres) {
    console.error('Error: provide a URL, or --scip <file>, or --postgres <dsn>');
    process.exitCode = 1;
    return;
  }
  try {
    if (opts.scip) {
      const counts = ingestScip(opts.graph, opts.scip);
      console.log(`SCIP index ingested: ${counts.nodesAdded} nodes, ${counts.edgesAdded} edges`);
      return;
    }
    if (opts.postgres) {
      const counts = ingestPostgres(opts.graph, opts.postgres);
      console.log(`Postgres schema ingested: ${counts.nodesAdded} nodes, ${counts.edgesAdded} edges`);
      return;
    }
    console.log(`Fetching ${url}...`);
    const result = ingestUrl(opts.graph, url, opts.author, opts.contributor);
    console.log(`Saved: ${result.savedPath}`);
    if (result.graphUpdated) {
      console.log('Graph updated with the new content.');
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
