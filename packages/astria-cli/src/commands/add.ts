import { readFileSync } from 'node:fs';
import { ingestUrl, ingestScip, ingestPostgres, saveTranscript } from '../native';

export async function addCommand(url: string, opts: {
  graph: string;
  author?: string;
  contributor?: string;
  scip?: string;
  postgres?: string;
  transcript?: string;
}) {
  if (!url && !opts.scip && !opts.postgres && !opts.transcript) {
    console.error('Error: provide a URL, or --scip <file>, or --postgres <dsn>, or --transcript <file|->');
    process.exitCode = 1;
    return;
  }
  try {
    if (opts.transcript) {
      // '-' reads piped stdin, so any transcription tool can stream
      // straight in: `whisper ... | astria add --transcript -`
      const fromStdin = opts.transcript === '-';
      const result = fromStdin
        ? saveTranscript(opts.graph, undefined, readFileSync(0, 'utf8'))
        : saveTranscript(opts.graph, opts.transcript, undefined);
      console.log(`Transcript saved: ${result.savedPath}`);
      if (result.graphUpdated) {
        console.log('Graph updated with the new content.');
      }
      return;
    }
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
