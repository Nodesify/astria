import { exportJsonCmd, exportHtmlCmd, exportGraphmlCmd, exportCypherCmd } from '../native';

export async function exportCommand(opts: { graph: string; out: string; format: string; mode?: string }) {
  try {
    const format = opts.format || 'json';

    if (format === 'html') {
      const outPath = opts.out.replace(/\.json$/, '.html');
      exportHtmlCmd(opts.graph, outPath, opts.mode || 'standard');
      console.log(`Exported HTML to: ${outPath}`);
    } else if (format === 'graphml') {
      const outPath = opts.out.replace(/\.json$/, '.graphml');
      exportGraphmlCmd(opts.graph, outPath);
      console.log(`Exported GraphML to: ${outPath}`);
    } else if (format === 'cypher') {
      const outPath = opts.out.replace(/\.json$/, '.cypher');
      const statements = exportCypherCmd(opts.graph, outPath);
      console.log(`Exported Cypher to: ${outPath} (${statements} MERGE statements — idempotent, safe to re-run)`);
    } else {
      exportJsonCmd(opts.graph, opts.out);
      console.log(`Exported JSON to: ${opts.out}`);
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
