import { diffGraphs } from '../native';

export async function diffCommand(pathA: string, pathB: string) {
  try {
    const result = diffGraphs(pathA, pathB);
    console.log(`Nodes added: ${result.nodesAdded}`);
    console.log(`Nodes removed: ${result.nodesRemoved}`);
    console.log(`Edges added: ${result.edgesAdded}`);
    console.log(`Edges removed: ${result.edgesRemoved}`);

    if (result.addedNodeLabels.length > 0) {
      console.log('\nAdded nodes:');
      for (const label of result.addedNodeLabels.slice(0, 20)) {
        console.log(`  + ${label}`);
      }
      if (result.addedNodeLabels.length > 20) {
        console.log(`  ... and ${result.addedNodeLabels.length - 20} more`);
      }
    }

    if (result.removedNodeLabels.length > 0) {
      console.log('\nRemoved nodes:');
      for (const label of result.removedNodeLabels.slice(0, 20)) {
        console.log(`  - ${label}`);
      }
      if (result.removedNodeLabels.length > 20) {
        console.log(`  ... and ${result.removedNodeLabels.length - 20} more`);
      }
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
