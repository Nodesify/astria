import { graphHistory } from '../native';

export async function historyCommand(opts: { limit: string; graph: string }) {
  try {
    const limit = parseInt(opts.limit || '20', 10);
    const entries = graphHistory(opts.graph, limit);
    if (entries.length === 0) {
      console.log('No query history found.');
      return;
    }
    console.log(`Recent queries (showing ${entries.length}):\n`);
    for (const entry of entries) {
      console.log(`[#${entry.id}] ${entry.question}`);
      if (entry.answer) {
        const preview = entry.answer.split('\n')[0];
        console.log(`  ${preview.substring(0, 100)}${preview.length > 100 ? '...' : ''}`);
      }
      console.log();
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
