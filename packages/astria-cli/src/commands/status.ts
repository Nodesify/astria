import { existsSync, statSync } from 'fs';
import { graphStats, graphBuildInfo } from '../native';

const STALE_THRESHOLD = 30;
const VERY_STALE_THRESHOLD = 120;

export async function statusCommand(opts: { graph: string; json?: boolean }) {
  const dbPath = `${opts.graph}/.astria/db.sqlite`;
  const graphJsonPath = `${opts.graph}/.astria/graph.json`;

  if (!existsSync(dbPath)) {
    if (opts.json) {
      console.log(JSON.stringify({ status: 'missing' }, null, 2));
    } else {
      console.log('Status: no graph found');
      console.log('Run `astria run .` to build the graph');
    }
    return;
  }

  let stats;
  try {
    stats = graphStats(opts.graph);
  } catch (e: any) {
    console.log('Status: error reading graph database');
    console.log(e.message || String(e));
    process.exitCode = 1;
    return;
  }

  if (stats.nodeCount === 0) {
    if (opts.json) {
      console.log(JSON.stringify({ status: 'empty', nodes: 0 }, null, 2));
    } else {
      console.log('Status: empty graph (0 nodes)');
      console.log('Run `astria run .` to populate the graph');
    }
    return;
  }

  // Build provenance (which astria and extraction rules built the graph);
  // graphs from before stamping simply report nulls.
  let build;
  try {
    build = graphBuildInfo(opts.graph);
  } catch {
    build = undefined;
  }

  if (!existsSync(graphJsonPath)) {
    if (opts.json) {
      console.log(
        JSON.stringify({ status: 'incomplete', nodes: stats.nodeCount, edges: stats.edgeCount }, null, 2),
      );
    } else {
      console.log(`Status: incomplete (db has ${stats.nodeCount} nodes but no graph.json)`);
      console.log('Run `astria run .` to complete the build');
    }
    return;
  }

  const mtime = statSync(graphJsonPath).mtimeMs;
  const ageMinutes = Math.round((Date.now() - mtime) / 60000);

  let staleness: string;
  if (ageMinutes <= STALE_THRESHOLD) {
    staleness = 'fresh';
  } else if (ageMinutes <= VERY_STALE_THRESHOLD) {
    staleness = 'stale';
  } else {
    staleness = 'very_stale';
  }

  // A graph extracted by older rules needs re-extraction even when the file
  // manifest is unchanged; the mismatch is the true freshness signal.
  const extractionOutdated =
    build?.extractionHashVersion != null &&
    build.extractionHashVersion !== build.currentExtractionHashVersion;

  if (opts.json) {
    console.log(
      JSON.stringify(
        {
          status: staleness,
          ageMinutes,
          nodes: stats.nodeCount,
          edges: stats.edgeCount,
          communities: stats.communityCount,
          files: stats.fileCount,
          builtAt: build?.graphPublishedAt ?? null,
          astriaVersion: build?.astriaVersion ?? null,
          extractionHashVersion: build?.extractionHashVersion ?? null,
          currentExtractionHashVersion: build?.currentExtractionHashVersion ?? null,
          extractionOutdated,
          staleExternalIndexes: build?.staleExternalIndexes ?? [],
        },
        null,
        2,
      ),
    );
    return;
  }

  console.log(`Status: ${staleness} (${ageMinutes} min ago)`);
  console.log(`Nodes: ${stats.nodeCount}`);
  console.log(`Edges: ${stats.edgeCount}`);
  console.log(`Communities: ${stats.communityCount}`);
  console.log(`Files tracked: ${stats.fileCount}`);
  if (build?.staleExternalIndexes?.length) {
    console.log(`Compiler indexes need reimport: ${build.staleExternalIndexes.join(', ')}`);
  }
  if (build?.graphPublishedAt) {
    // Stored as unix seconds; render ISO so humans can read it.
    const when = new Date(Number(build.graphPublishedAt) * 1000);
    const builtAt = Number.isFinite(when.getTime()) ? when.toISOString() : build.graphPublishedAt;
    const by = build.astriaVersion ? ` by astria ${build.astriaVersion}` : '';
    const extraction = build.extractionHashVersion ? ` (extraction ${build.extractionHashVersion})` : '';
    console.log(`Built: ${builtAt}${by}${extraction}`);
  }
  if (extractionOutdated) {
    console.log(
      `Warning: graph was built with extraction ${build?.extractionHashVersion}; this astria uses ${build?.currentExtractionHashVersion} — re-extract with \`astria update .\``,
    );
    return;
  }
  if (staleness === 'stale' || staleness === 'very_stale') {
    console.log(`Recommendation: run \`astria update .\` to refresh`);
  }
}
