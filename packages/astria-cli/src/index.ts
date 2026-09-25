#!/usr/bin/env node

import { Command } from 'commander';
import { runCommand } from './commands/run';
import { statsCommand } from './commands/stats';
import { explainCommand } from './commands/explain';
import { exportCommand } from './commands/export';
import { queryCommand } from './commands/query';
import { pathCommand } from './commands/path';
import { mapCommand } from './commands/map';
import { affectedCommand } from './commands/affected';
import { mcpCommand } from './commands/mcp';
import { treeCommand } from './commands/tree';
import { wikiCommand } from './commands/wiki';
import { prsCommand } from './commands/prs';
import { addCommand } from './commands/add';
import { diagnoseCommand } from './commands/diagnose';
import { saveResultCommand, reflectCommand } from './commands/feedback';
import { globalAddCommand, globalRemoveCommand, globalListCommand, globalPathCommand } from './commands/global';
import { hookGuard } from './commands/hook-guard';
import { updateCommand } from './commands/update';
import { watchCommand } from './commands/watch';
import { clusterCommand } from './commands/cluster';
import { mergeCommand } from './commands/merge';
import { diffCommand } from './commands/diff';
import { historyCommand } from './commands/history';
import { statusCommand } from './commands/status';
import { registerInstallCommand } from './commands/install';
import { registerHookCommand } from './commands/hook';

const program = new Command();

program
  .name('astria')
  .description('Turn any folder into a queryable knowledge graph')
  .version(require('../package.json').version);

program
  .command('run')
  .description('Run the full pipeline on a directory')
  .argument('<path>', 'Directory to analyze')
  .option('--no-dedup', 'Skip near-duplicate node merging')
  .option('--backend <name>', 'Semantic LLM backend: claude, openai (any OpenAI-compatible), or gemini')
  .option('--model <name>', 'Semantic LLM model name (backend-specific)')
  .option('--wiki', 'Also export a markdown wiki to .graphify/wiki')
  .option('--embed', 'Compute local embeddings: similar_to edges + semantic query recall (downloads a small model on first use)')
  .option('--global', 'After building, merge this repo into the cross-repo global graph')
  .option('--as <tag>', 'Repo tag for --global (defaults to the directory name)')
  .action((path, opts) => runCommand(path, { ...opts, global: opts.global, as: opts.as }));

program
  .command('update')
  .description('Run incremental AST-only rebuild')
  .argument('<path>', 'Directory to update')
  .option('--no-dedup', 'Skip near-duplicate node merging')
  .option('--backend <name>', 'Semantic LLM backend: claude, openai (any OpenAI-compatible), or gemini')
  .option('--model <name>', 'Semantic LLM model name (backend-specific)')
  .option('--embed', 'Compute local embeddings: similar_to edges + semantic query recall (downloads a small model on first use)')
  .action(updateCommand);

program
  .command('watch')
  .description('Watch for file changes and auto-rebuild')
  .argument('<path>', 'Directory to watch')
  .option('--debounce <ms>', 'Debounce interval in milliseconds', '3000')
  .action(watchCommand);

program
  .command('explain')
  .description('Explain a node and its connections')
  .argument('<node>', 'Node ID or label')
  .option('--graph <path>', 'Path to project root', '.')
  .action(explainCommand);

program
  .command('query')
  .description('BFS/DFS graph traversal for a question')
  .argument('<question>', 'Search terms')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--dfs', 'Use depth-first search instead of breadth-first')
  .option('--depth <n>', 'Traversal depth', '2')
  .option('--budget <n>', 'Token budget for output', '2000')
  .option('--directed', 'Follow edges only in their stored direction (caller -> callee)')
  .option('--detail <level>', 'Fidelity tier: "high" keeps only EXTRACTED/DECLARED facts')
  .option('--cursor <n>', 'Continuation token from a previous truncated query', '0')
  .action(queryCommand);

program
  .command('path')
  .description('Find shortest path between two nodes')
  .argument('<source>', 'Source node label')
  .argument('<target>', 'Target node label')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--directed', 'Follow edges only in their stored direction (caller -> callee)')
  .option('--detail <level>', 'Fidelity tier: "high" keeps only EXTRACTED/DECLARED facts')
  .action(pathCommand);

program
  .command('map')
  .description('Repo map: PageRank-ranked files with top symbols, within a token budget')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--budget <n>', 'Token budget for output', '2000')
  .option('--detail <level>', 'Fidelity tier: "high" keeps only EXTRACTED/DECLARED facts')
  .action(mapCommand);

program
  .command('affected')
  .description('Show the blast radius of a node — everything impacted by changing it')
  .argument('<node>', 'Node ID, label, or source file path')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--depth <n>', 'Maximum hops to traverse', '2')
  .option('--relation <type>', 'Only follow one relation (e.g. calls, imports, uses)')
  .action(affectedCommand);

program
  .command('stats')
  .description('Show graph statistics')
  .option('--graph <path>', 'Path to project root', '.')
  .action(statsCommand);

program
  .command('export')
  .description('Export graph to JSON, HTML, GraphML, or Cypher (Neo4j)')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--out <file>', 'Output file', 'graph.json')
  .option('--format <type>', 'Export format: json, html, graphml, cypher', 'json')
  .option('--mode <mode>', 'HTML visualization mode: standard or large', 'standard')
  .action(exportCommand);

program
  .command('cluster-only')
  .description('Run cluster + analyze + report only (no extract/build)')
  .argument('<path>', 'Directory with existing graph')
  .action(clusterCommand);

program
  .command('merge')
  .description('Merge two graphs into a new output graph')
  .argument('<pathA>', 'First project root')
  .argument('<pathB>', 'Second project root')
  .argument('<outPath>', 'Output project root')
  .action(mergeCommand);

program
  .command('diff')
  .description('Compare two graphs and show differences')
  .argument('<pathA>', 'First project root')
  .argument('<pathB>', 'Second project root')
  .action(diffCommand);

program
  .command('history')
  .description('Show recent query history')
  .option('--limit <n>', 'Number of entries to show', '20')
  .option('--graph <path>', 'Path to project root', '.')
  .action(historyCommand);

program
  .command('mcp')
  .description('Run an MCP stdio server exposing the graph to AI agents (Claude, etc.)')
  .option('--graph <path>', 'Path to project root', '.')
  .action(mcpCommand);

program
  .command('tree')
  .description('Export a collapsible filesystem tree of all graph symbols (self-contained HTML)')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--out <file>', 'Output HTML file', 'tree.html')
  .option('--max-children <n>', 'Max symbols shown per directory', '40')
  .action(treeCommand);

program
  .command('wiki')
  .description('Export a Wikipedia-style markdown wiki (index.md + one article per community and god node)')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--out <dir>', 'Output directory', '.graphify/wiki')
  .option('--max-nodes <n>', 'Max key concepts listed per community article', '25')
  .option('--format <type>', 'markdown (wiki articles) or obsidian (vault: per-node notes + canvas)', 'markdown')
  .action(wikiCommand);

program
  .command('prs')
  .description('Map open pull requests onto the knowledge graph (impact + merge-order risk)')
  .argument('[count]', 'Number of PRs to analyze', '20')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--conflicts', 'Flag PRs sharing communities (merge-order risk)')
  .action(prsCommand);

program
  .command('add')
  .description('Fetch a URL (arXiv paper, tweet, webpage, image, PDF) into ./raw and update the graph')
  .argument('[url]', 'URL to fetch (required unless --scip/--postgres is given)')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--author <name>', 'Author recorded in the saved metadata')
  .option('--contributor <name>', 'Contributor recorded in the saved metadata')
  .option('--scip <file>', 'Ingest a simplified SCIP JSON index instead of fetching a URL')
  .option('--postgres <dsn>', 'Introspect a live PostgreSQL schema (requires psql on PATH) instead of fetching a URL')
  .action(addCommand);

program
  .command('status')
  .description('Check graph health and staleness')
  .option('--graph <path>', 'Path to project root', '.')
  .action(statusCommand);

registerInstallCommand(program);
registerHookCommand(program);


program
  .command('diagnose')
  .description('Read-only graph health report: dangling edges, self-loops, duplicates, stubs')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--json', 'Machine-readable output')
  .action(diagnoseCommand);

program
  .command('save-result')
  .description('Save a Q/A pair into the graph memory for future runs')
  .argument('<question>', 'The question that was asked')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--answer <text>', 'The answer to record')
  .option('--answer-file <path>', 'Read the answer from a file')
  .option('--outcome <kind>', 'useful | dead_end | corrected')
  .option('--correction <text>', 'Corrections to the recorded answer')
  .option('--nodes <ids>', 'Comma-separated source node ids this answer cites')
  .action(saveResultCommand);

program
  .command('reflect')
  .description('Aggregate memory outcomes into .graphify/reflections/LESSONS.md')
  .option('--graph <path>', 'Path to project root', '.')
  .action(reflectCommand);

const globalCmd = program
  .command('global')
  .description('Cross-repo global graph: merge many repo graphs into one queryable store');

globalCmd
  .command('add')
  .description('Merge a repo graph into the global store (idempotent by tag)')
  .argument('<path>', 'Repo root with a .graphify directory')
  .option('--as <tag>', 'Repo tag (defaults to the directory name)')
  .action(globalAddCommand);

globalCmd
  .command('remove')
  .description('Remove a repo from the global graph')
  .argument('<tag>', 'Repo tag')
  .action(globalRemoveCommand);

globalCmd
  .command('list')
  .description('List repos registered in the global graph')
  .action(() => globalListCommand());

globalCmd
  .command('path')
  .description('Shortest path across repos in the global graph')
  .argument('<source>', 'Source node id or unique label')
  .argument('<target>', 'Target node id or unique label')
  .action(globalPathCommand);

program
  .command('hook-guard')
  .description('Editor PreToolUse guard (installed into .claude/settings.json) — internal use')
  .argument('<mode>', 'search | read | gemini')
  .allowUnknownOption(true)
  .action((mode: string) => {
    hookGuard(mode, process.argv.slice(4));
  });

if (require.main === module) {
  program.parse();
}

export { program };
