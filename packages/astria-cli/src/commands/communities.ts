import { listCommunities } from '../native';

export async function communitiesCommand(opts: { graph: string; json?: boolean }) {
  try {
    const result = listCommunities(opts.graph);
    if (opts.json) {
      console.log(JSON.stringify(result, null, 2));
      return;
    }
    const modularity = result.modularity != null ? ` (modularity ${result.modularity.toFixed(3)})` : '';
    const llmNamed = result.communities.filter((c: any) => c.labelSource === 'llm').length;
    console.log(`${result.communities.length} communities${modularity}:`);
    for (const c of result.communities) {
      // An LLM label is a summary of the code; a hub label is a fact about it.
      const sourceTag = c.labelSource === 'llm' ? '' : ' [hub]';
      const summary = c.summary ? ` — ${c.summary}` : '';
      const cohesion = c.cohesion != null ? c.cohesion.toFixed(2) : '-';
      console.log(`  [${c.id}] ${c.label}${sourceTag} - ${c.size} nodes, cohesion ${cohesion}${summary}`);
    }
    if (llmNamed > 0) {
      console.log(`\n${llmNamed} communities carry LLM thematic labels (astria run --label-communities).`);
    }
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
