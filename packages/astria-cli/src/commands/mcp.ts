import { runMcpServer } from '../native';

export async function mcpCommand(opts: { graph: string }) {
  try {
    // Blocks serving newline-delimited JSON-RPC on stdio until stdin closes
    runMcpServer(opts.graph);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
