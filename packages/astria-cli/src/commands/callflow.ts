import { writeFileSync } from 'fs';
import { callflowMermaid } from '../native';

export async function callflowCommand(
  node: string,
  opts: { graph: string; depth?: string; direction?: string; out?: string },
) {
  try {
    const depth = Math.max(1, Number(opts.depth ?? 2) || 2);
    const direction = opts.direction || 'out';
    if (!['in', 'out', 'both'].includes(direction)) {
      throw new Error(`Unknown direction "${direction}". Valid: in, out, both`);
    }
    const mermaid = callflowMermaid(opts.graph, node, depth, direction);
    if (opts.out) {
      writeFileSync(opts.out, mermaid + '\n');
      console.log(`Call-flow written to: ${opts.out}`);
    } else {
      console.log(mermaid);
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
