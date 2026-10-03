import * as pathMod from 'path';
import { clusterOnly } from '../native';

export interface ClusterOnlyOptions {
  /** Community granularity, 0.0–1.0. Higher → more, smaller communities. */
  resolution?: string;
  /** Keep high-degree hubs from gluing communities together. */
  excludeHubs?: boolean;
}

export async function clusterCommand(path: string, opts: ClusterOnlyOptions = {}) {
  try {
    let resolution: number | undefined;
    if (opts.resolution !== undefined) {
      resolution = parseFloat(opts.resolution);
      if (!Number.isFinite(resolution) || resolution < 0 || resolution > 1) {
        throw new Error('--resolution must be a number between 0.0 and 1.0');
      }
    }
    console.log(`Running cluster + analyze on: ${path}`);
    const result = clusterOnly(path, resolution, opts.excludeHubs === true);
    console.log(`Communities: ${result.communities}`);
    console.log(`Report updated at: ${pathMod.join(path, '.astria', 'graph_report.md')}`);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
