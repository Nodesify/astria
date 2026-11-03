import { existsSync } from 'fs';
import { join } from 'path';
import { graphStats, graphBuildInfo, graphFreshness } from '../native';

export async function statusCommand(opts: { graph: string; json?: boolean }) {
  try {
    if (!existsSync(join(opts.graph, '.astria', 'db.sqlite'))) {
      console.log(opts.json ? JSON.stringify({ status: 'missing' }) : 'Status: missing — run `astria run .`');
      return;
    }
    const stats = graphStats(opts.graph);
    const build = graphBuildInfo(opts.graph);
    const freshness = graphFreshness(opts.graph);
    const published = Number(freshness.graphBuiltAt);
    const ageMinutes = freshness.graphBuiltAt && Number.isFinite(published)
      ? Math.max(0, (Date.now() / 1000 - published) / 60) : null;
    const recommendations: string[] = [];
    if (stats.nodeCount === 0 || freshness.status !== 'fresh') recommendations.push('Run astria update with this project path to refresh structural evidence.');
    if (freshness.staleExternalIndexes.length) recommendations.push('Regenerate each stale compiler index from current source, then run astria add --scip <index-file> with this project as --graph. Structural update does not regenerate compiler indexes.');
    const result = {
      ...freshness,
      recommendations,
      status: stats.nodeCount === 0 ? 'empty' : freshness.status,
      ageMinutes,
      nodes: stats.nodeCount, edges: stats.edgeCount, communities: stats.communityCount, files: stats.fileCount,
      builtAt: freshness.graphBuiltAt,
      astriaVersion: build.astriaVersion,
      extractionHashVersion: build.extractionHashVersion,
      currentExtractionHashVersion: build.currentExtractionHashVersion,
    };
    if (opts.json) console.log(JSON.stringify(result, null, 2));
    else {
      console.log(`Status: ${result.status}`);
      console.log(`Source changes: ${freshness.added} added, ${freshness.modified} modified, ${freshness.deleted} deleted`);
      console.log(`Graph age: ${ageMinutes === null ? 'unknown' : `${Math.round(ageMinutes)} min`}`);
      console.log(`Nodes: ${stats.nodeCount}; edges: ${stats.edgeCount}; communities: ${stats.communityCount}; files: ${stats.fileCount}`);
      console.log(`Artifacts: ${freshness.artifactsConsistent ? 'consistent' : 'incomplete or mismatched'}`);
      console.log(`Source check: ${freshness.filesChecked} files, ${freshness.checkMilliseconds} ms (content hashes)`);
      if (freshness.extractionOutdated) console.log('Extraction rules changed; run `astria update .`');
      if (freshness.staleExternalIndexes.length) console.log(`Compiler indexes need reimport: ${freshness.staleExternalIndexes.join(', ')}`);
      if (freshness.error) console.log(`Freshness unavailable: ${freshness.error}`);
      for (const recommendation of recommendations) console.log(`Recommendation: ${recommendation}`);
    }
    if (freshness.error) process.exitCode = 1;
  } catch (error: any) {
    if (opts.json) console.log(JSON.stringify({ status: 'unknown', error: error.message || String(error) }));
    else console.error(`Error reading graph status: ${error.message || error}`);
    process.exitCode = 1;
  }
}
