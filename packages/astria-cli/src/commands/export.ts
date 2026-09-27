import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'fs';
import { exportJsonCmd, exportHtmlCmd, exportGraphmlCmd, exportCypherCmd, exportSvgCmd, neo4jPushCmd } from '../native';

export async function exportCommand(opts: { graph: string; out: string; format: string; mode?: string; neo4jPush?: string; neo4jUser?: string; neo4jPass?: string; redisPush?: string; graphName?: string }) {
  try {
    const format = opts.format || 'json';

    if (!['json', 'html', 'graphml', 'cypher', 'svg', 'falkordb'].includes(format)) {
      const hint = format === 'obsidian' ? ' (obsidian is a wiki format: astria wiki --format obsidian)' : '';
      throw new Error(`Unknown export format "${format}". Valid formats: json, html, graphml, cypher, svg, falkordb${hint}`);
    }

    // Live Neo4j push: same graph data the Cypher file carries, straight
    // over Bolt. Credentials come from flags or NEO4J_USERNAME/NEO4J_PASSWORD.
    if (opts.neo4jPush) {
      if (format !== 'cypher') {
        throw new Error('--neo4j-push pairs with --format cypher (it pushes the same graph live)');
      }
      const counts = neo4jPushCmd(opts.graph, opts.neo4jPush, opts.neo4jUser ?? null, opts.neo4jPass ?? null);
      console.log(`Pushed to Neo4j at ${opts.neo4jPush}: ${counts.nodes} nodes, ${counts.edges} edges, ${counts.communities} communities (${counts.statements} statements)`);
      return;
    }

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
    } else if (format === 'falkordb') {
      const outPath = opts.out.replace(/\.json$/, '.falkordb.cypher');
      const statements = exportCypherCmd(opts.graph, outPath);
      const body = readFileSync(outPath, 'utf-8');
      const NL = String.fromCharCode(10);
      const header =
        '// astria FalkorDB export - openCypher, idempotent (MERGE)' + NL +
        '// Load into a running FalkorDB/Redis:' + NL +
        `//   redis-cli -h <host> -p <port> -x GRAPH.QUERY ${opts.graphName || 'astria'} < statement.cypher` + NL +
        '// Or re-run this export with --redis-push <host:port> to push directly (requires redis-cli).' + NL;
      writeFileSync(outPath, header + body);
      console.log(`Exported FalkorDB Cypher to: ${outPath} (${statements} MERGE statements)`);

      if (opts.redisPush) {
        const [host, port] = opts.redisPush.split(':');
        const stmts = body.split(NL).filter((l) => l.trim() && !l.startsWith('//')).map((l) => l.replace(/;\s*$/, ''));
        let sent = 0;
        for (const stmt of stmts) {
          const r = spawnSync('redis-cli', ['-h', host, '-p', port || '6379', '-x', `GRAPH.QUERY ${opts.graphName || 'astria'}`], {
            input: stmt,
            encoding: 'utf8',
          });
          if (r.status !== 0) {
            throw new Error(`redis-cli failed on statement ${sent + 1}: ${r.stderr || r.stdout || 'unknown error'}`);
          }
          sent++;
        }
        console.log(`Pushed ${sent} statements to FalkorDB graph "${opts.graphName || 'astria'}"`);
      }
      return;
    } else if (format === 'svg') {
      const outPath = opts.out.replace(/\.json$/, '.svg');
      const counts = exportSvgCmd(opts.graph, outPath);
      console.log(`Exported SVG to: ${outPath} (${counts.nodes} nodes, ${counts.edges} edges, ${counts.communities} communities${counts.truncated ? ', capped' : ''})`);
    } else {
      exportJsonCmd(opts.graph, opts.out);
      console.log(`Exported JSON to: ${opts.out}`);
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
