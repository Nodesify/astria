import { globalAdd, globalRemove, globalList, globalPath } from '../native';

export async function globalAddCommand(path: string, opts: { as?: string }) {
  try {
    const result = globalAdd(path, opts.as);
    console.log(`Repo '${result.tag}' merged into the global graph.`);
    console.log(
      `Nodes: ${result.nodesAdded} | Edges: ${result.edgesAdded} | ` +
        `same_type_as: ${result.sameTypeEdges} | cross-repo calls: ${result.crossRepoCallEdges}`
    );
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}

export async function globalRemoveCommand(tag: string) {
  try {
    const removed = globalRemove(tag);
    console.log(`Removed '${tag}' (${removed} nodes) from the global graph.`);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}

export async function globalListCommand() {
  try {
    const entries = globalList();
    if (entries.length === 0) {
      console.log('Global graph is empty. Add repos with: astria global add <path>');
      return;
    }
    console.log('Global graph repos:');
    for (const e of entries) {
      console.log(`  ${e.tag} — ${e.nodes} nodes, ${e.edges} edges`);
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}

export async function globalPathCommand(source: string, target: string) {
  try {
    const result = globalPath(source, target);
    if (!result) {
      console.log(`No path found between '${source}' and '${target}'.`);
      return;
    }
    console.log(result.replace(/ --(?=[^\s])/g, ' --\n  '));
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
