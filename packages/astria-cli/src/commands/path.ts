import { findPath } from '../native';

export async function pathCommand(source: string, target: string, opts: {
  graph: string;
  directed?: boolean;
  detail?: string;
  json?: boolean;
}) {
  try {
    const result = findPath(opts.graph, source, target, opts.directed ?? false, opts.detail);
    if (opts.json) {
      console.log(
        JSON.stringify(
          { source, target, found: result.found, hops: result.hops, text: result.text },
          null,
          2,
        ),
      );
      return;
    }
    console.log(result.text);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
