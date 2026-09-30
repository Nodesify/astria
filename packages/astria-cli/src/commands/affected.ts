import { affectedNode } from '../native';

export async function affectedCommand(node: string, opts: {
  graph: string;
  depth: string;
  relation?: string;
  json?: boolean;
}) {
  try {
    const depth = parseInt(opts.depth, 10) || 2;
    const result = affectedNode(opts.graph, node, depth, opts.relation);
    if (opts.json) {
      console.log(
        JSON.stringify(
          {
            seed: result.seed,
            seedLabel: result.seedLabel,
            total: result.total,
            hits: result.hits,
          },
          null,
          2,
        ),
      );
      return;
    }
    if (result.total === 0) {
      console.log(`Nothing references "${result.seedLabel}" — no blast radius.`);
      return;
    }
    console.log(`Blast radius of "${result.seedLabel}" (depth ≤ ${depth}): ${result.total} node(s)`);
    console.log();
    let lastDepth = 0;
    let sawInferred = false;
    for (const hit of result.hits) {
      if (hit.depth !== lastDepth) {
        lastDepth = hit.depth;
        console.log(`  depth ${hit.depth}:`);
      }
      // INFERRED hops have no source locus (or a name too common to bind);
      // show them as weaker evidence. RESOLVED hops (call extracted from
      // source, unique binding) are trustworthy.
      const provenance =
        hit.provenance && hit.provenance !== 'EXTRACTED' ? ` ${hit.provenance}` : '';
      if (hit.provenance === 'INFERRED') sawInferred = true;
      const via = hit.viaFile ? `  [${hit.viaFile}]` : '';
      console.log(`    ${hit.label} (${hit.relation}${provenance})${via}`);
    }
    if (sawInferred) {
      console.log();
      console.log('  (hits marked INFERRED have no source locus — reconstructed from name references)');
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
