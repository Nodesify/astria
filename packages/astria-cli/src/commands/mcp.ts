import { runMcpServer, runMcpHttpServer } from '../native';

export interface McpOptions {
  graph: string;
  /** Serve MCP over HTTP (Streamable HTTP, JSON responses) instead of stdio. */
  http?: boolean;
  host?: string;
  port?: string;
  /** Bearer token for HTTP serving (falls back to ASTRIA_MCP_TOKEN). */
  token?: string;
  /** Additional projects: "name=path" or bare "path" entries. */
  projects?: string[];
  /**
   * Browser origins allowed to send HTTP requests (exact match, e.g.
   * "http://localhost:5173"). Requests carrying an Origin header are refused
   * unless listed; native MCP clients send no Origin and always pass.
   */
  allowOrigin?: string[];
}

export async function mcpCommand(opts: McpOptions) {
  try {
    if (opts.http) {
      const port = opts.port ? parseInt(opts.port, 10) : undefined;
      if (port !== undefined && (!Number.isInteger(port) || port <= 0 || port > 65535)) {
        throw new Error(`invalid port: ${opts.port}`);
      }
      // Blocks until the process is killed; one server, many project graphs.
      runMcpHttpServer({
        root: opts.graph,
        host: opts.host,
        port,
        token: opts.token,
        projects: opts.projects,
        allowedOrigins: opts.allowOrigin,
      });
      return;
    }
    // Blocks serving newline-delimited JSON-RPC on stdio until stdin closes
    runMcpServer(opts.graph);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
