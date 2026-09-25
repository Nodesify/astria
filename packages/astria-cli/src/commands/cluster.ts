import * as pathMod from 'path';
import { clusterOnly } from '../native';

export async function clusterCommand(path: string) {
  try {
    console.log(`Running cluster + analyze on: ${path}`);
    const result = clusterOnly(path);
    console.log(`Communities: ${result.communities}`);
    console.log(`Report updated at: ${pathMod.join(path, '.graphify', 'graph_report.md')}`);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
