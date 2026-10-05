#!/usr/bin/env node

import { Command } from 'commander';
import { DEFAULT_QUERY_BUDGET } from './defaults';
import { runCommand } from './commands/run';
import { healthCommand } from './commands/health';
import { riskCommand } from './commands/risk';
import { statsCommand } from './commands/stats';
import { godNodesCommand } from './commands/god-nodes';
import { communitiesCommand } from './commands/communities';
import { neighborsCommand } from './commands/neighbors';
import { explainCommand } from './commands/explain';
import { exportCommand } from './commands/export';
import { callflowCommand } from './commands/callflow';
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
import { mergeDriverInstall, mergeDriverRun, mergeDriverUninstall } from './commands/merge-driver';
import { digestCommand } from './commands/digest';
import { mergeGateCommand } from './commands/merge-gate';

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
  .option('--backend <name>', 'Semantic LLM backend: claude, openai (any OpenAI-compatible), azure (Azure OpenAI), bedrock (AWS SigV4), kimi (Moonshot), or gemini')
  .option('--judge <name>', 'Decision layer over the backend: jev (TypeSafe System One — gates trivial files, re-judges relations/node types, adds calibrated edge confidence)')
  .option('--model <name>', 'Semantic LLM model name (backend-specific)')
  .option('--wiki', 'Also export a markdown wiki to .astria/wiki')
  .option('--embed', 'Compute local embeddings: similar_to edges + semantic query recall (one-time ~615 MB local model download, then offline)')
  .option('--label-communities', 'Name communities thematically with one LLM call per changed community (requires a semantic backend)')
  .option('--deep', 'Second extraction tier: LLM-linked cross-file concept edges, cached per file (requires a semantic backend)')
  .option('--global', 'After building, merge this repo into the cross-repo global graph')
  .option('--as <tag>', 'Repo tag for --global (defaults to the directory name)')
  .action((path, opts) => runCommand(path, { ...opts, global: opts.global, as: opts.as }));

program
  .command('update')
  .description('Run incremental AST-only rebuild')
  .argument('<path>', 'Directory to update')
  .option('--no-dedup', 'Skip near-duplicate node merging')
  .option('--backend <name>', 'Semantic LLM backend: claude, openai (any OpenAI-compatible), azure (Azure OpenAI), bedrock (AWS SigV4), kimi (Moonshot), or gemini')
  .option('--judge <name>', 'Decision layer over the backend: jev (TypeSafe System One — gates trivial files, re-judges relations/node types, adds calibrated edge confidence)')
  .option('--model <name>', 'Semantic LLM model name (backend-specific)')
  .option('--embed', 'Compute local embeddings: similar_to edges + semantic query recall (one-time ~615 MB local model download, then offline)')
  .option('--label-communities', 'Name communities thematically with one LLM call per changed community (requires a semantic backend)')
  .option('--deep', 'Second extraction tier: LLM-linked cross-file concept edges, cached per file (requires a semantic backend)')
  .option('--quiet', 'Suppress progress lines and the token benchmark')
  .option('--if-stale <minutes>', 'Skip when the graph was updated less than N minutes ago')
  .action((path, opts) =>
    updateCommand(path, { ...opts, ifStale: opts.ifStale ? Number(opts.ifStale) : undefined }),
  );

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
  .option('--json', 'Emit machine-readable JSON')
  .action(explainCommand);

program
  .command('query')
  .description('BFS/DFS graph traversal for a question')
  .argument('<question>', 'Search terms')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--dfs', 'Use depth-first search instead of breadth-first')
  .option('--depth <n>', 'Traversal depth', '2')
  .option('--budget <n>', 'Token budget for output', String(DEFAULT_QUERY_BUDGET))
  .option('--directed', 'Follow edges only in their stored direction (caller -> callee)')
  .option('--detail <level>', 'Fidelity tier: "high" keeps only EXTRACTED/DECLARED facts')
  .option('--cursor <n>', 'Continuation token from a previous truncated query', '0')
  .option('--no-embed', 'Skip auto-merged embedding seeds even when the graph has vectors')
  .option('--json', 'Emit machine-readable JSON')
  .action(queryCommand);

program
  .command('path')
  .description('Find shortest path between two nodes')
  .argument('<source>', 'Source node label')
  .argument('<target>', 'Target node label')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--directed', 'Follow edges only in their stored direction (caller -> callee)')
  .option('--detail <level>', 'Fidelity tier: "high" keeps only EXTRACTED/DECLARED facts')
  .option('--json', 'Emit machine-readable JSON')
  .action(pathCommand);

program
  .command('map')
  .description('Repo map: PageRank-ranked files with top symbols, within a token budget')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--budget <n>', 'Token budget for output', String(DEFAULT_QUERY_BUDGET))
  .option('--detail <level>', 'Fidelity tier: "high" keeps only EXTRACTED/DECLARED facts')
  .option('--json', 'Emit machine-readable JSON')
  .action(mapCommand);

program
  .command('affected')
  .description('Show the blast radius of a node — everything impacted by changing it')
  .argument('<node>', 'Node ID, label, or source file path')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--depth <n>', 'Maximum hops to traverse', '2')
  .option('--relation <type>', 'Only follow one relation (e.g. calls, imports, uses)')
  .option('--json', 'Emit machine-readable JSON')
  .action(affectedCommand);

program
  .command('stats')
  .description('Show graph statistics')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--json', 'Emit machine-readable JSON')
  .action(statsCommand);

program
  .command('god-nodes')
  .description('List the highest-degree hub nodes (CLI parity with the MCP god_nodes tool)')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--json', 'Emit machine-readable JSON')
  .action(godNodesCommand);

program
  .command('communities')
  .description('List communities with labels, size, cohesion, and modularity (CLI parity with the MCP list_communities tool)')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--json', 'Emit machine-readable JSON')
  .action(communitiesCommand);

program
  .command('neighbors')
  .description('List a node\'s neighbors, optionally filtered by relation (CLI parity with the MCP get_neighbors tool)')
  .argument('<node>', 'Node ID or label')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--relation <type>', 'Only include edges with this relation')
  .option('--json', 'Emit machine-readable JSON')
  .action(neighborsCommand);

program
  .command('health')
  .description('Code-health report: unreachable-symbol candidates, file cycles, hub concentration, staleness (heuristic score)')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--json', 'Emit machine-readable JSON')
  .option('--min-score <n>', 'Exit non-zero when the health score is below this (CI gate)')
  .action(healthCommand);

program
  .command('risk')
  .description('Blast radius of the current git diff: impacted symbols and communities, with a heuristic risk score for PRs')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--staged', 'Only staged changes (git diff --cached) instead of the whole working tree')
  .option('--json', 'Emit machine-readable JSON')
  .action(riskCommand);

program
  .command('export')
  .description('Export graph to JSON, HTML, GraphML, SVG, Cypher (Neo4j), or FalkorDB Cypher')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--out <file>', 'Output file', 'graph.json')
  .option('--format <type>', 'Export format: json, html, graphml, cypher, svg, falkordb', 'json')
  .option('--mode <mode>', 'HTML visualization mode: standard or large', 'standard')
  .option('--neo4j-push <url>', 'Push to a live Neo4j (bolt://host:port) instead of writing a file — pairs with --format cypher')
  .option('--neo4j-user <user>', 'Neo4j username (default: neo4j or NEO4J_USERNAME)')
  .option('--neo4j-pass <pass>', 'Neo4j password (default: NEO4J_PASSWORD)')
  .option('--redis-push <host:port>', 'Push FalkorDB Cypher to a live FalkorDB/Redis (host:port) — pairs with --format falkordb; requires redis-cli')
  .option('--graph-name <name>', 'FalkorDB graph name', 'astria')
  .action(exportCommand);

program
  .command('callflow')
  .description('Mermaid call-flow diagram: what a node calls (or what calls it)')
  .argument('<node>', 'Node id, exact label, or bare name')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--depth <n>', 'Traversal depth', '2')
  .option('--direction <dir>', 'out (what it calls), in (what calls it), both', 'out')
  .option('--out <file>', 'Write the mermaid block to a file instead of stdout')
  .action(callflowCommand);

program
  .command('cluster-only')
  .description('Run cluster + analyze + report only (no extract/build)')
  .argument('<path>', 'Directory with existing graph')
  .option('--resolution <r>', 'Community granularity 0.0–1.0: higher values produce more, smaller communities', '0')
  .option('--exclude-hubs', 'Keep high-degree hub nodes out of propagation so they cannot glue communities together (hubs are attached to their strongest community afterwards)')
  .action((path: string, opts: { resolution: string; excludeHubs?: boolean }) => clusterCommand(path, { resolution: opts.resolution, excludeHubs: opts.excludeHubs }));

program
  .command('merge')
  .description('Merge two graphs into a new output graph (cross-repo by default: ids namespaced per root; --same-repo shares ids and errors on conflicts)')
  .argument('<pathA>', 'First project root')
  .argument('<pathB>', 'Second project root')
  .argument('<outPath>', 'Output project root')
  .option('--same-repo', 'Treat both inputs as one repository: no id namespacing; differing definitions under the same id fail the merge')
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
  .description('Run an MCP server exposing the graph to AI agents — stdio by default, or HTTP with --http')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--http', 'Serve MCP over HTTP (Streamable HTTP) instead of stdio — one server, many clients')
  .option('--host <addr>', 'HTTP bind address (default 127.0.0.1; non-loopback requires a token)')
  .option('--port <n>', 'HTTP port', '8620')
  .option('--token <token>', 'Bearer token for HTTP serving (default: ASTRIA_MCP_TOKEN; required off-loopback)')
  .option('--allow-origin <origin...>', 'Browser origins allowed to send HTTP requests, e.g. "http://localhost:5173" (requests with an Origin header are refused unless listed; native clients send none)')
  .option('--projects <paths...>', 'Additional projects to serve: "name=path" or "path" (project name defaults to the directory name)')
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
  .option('--out <dir>', 'Output directory', '.astria/wiki')
  .option('--max-nodes <n>', 'Max key concepts listed per community article', '25')
  .option('--format <type>', 'markdown (wiki articles) or obsidian (vault: per-node notes + canvas)', 'markdown')
  .action(wikiCommand);

program
  .command('prs')
  .description('Map open pull requests onto the knowledge graph: CI state, review status, worktree mapping, ranked review queue, merge-order risk')
  .argument('[count]', 'Number of PRs to analyze', '20')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--conflicts', 'Flag PRs sharing communities (merge-order risk)')
  .option('--triage', 'Compact per-PR triage lines instead of the full dashboard')
  .option('--queue', 'Print only the ranked review queue')
  .option('--json', 'Emit machine-readable JSON (ranked queue with all signals)')
  .action(prsCommand);

program
  .command('add')
  .description('Fetch a URL (arXiv paper, tweet, webpage, image, PDF) into ./raw, or save a transcript, and update the graph')
  .argument('[url]', 'URL to fetch (required unless --scip/--postgres/--transcript is given)')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--author <name>', 'Author recorded in the saved metadata')
  .option('--contributor <name>', 'Contributor recorded in the saved metadata')
  .option('--scip <file>', 'Ingest a simplified SCIP JSON index instead of fetching a URL')
  .option('--postgres <dsn>', 'Introspect a live PostgreSQL schema (requires psql on PATH) instead of fetching a URL')
  .option('--transcript <file>', 'Save a transcript (.md/.txt file, or - to read piped stdin) into .astria/transcripts/ and update the graph')
  .action(addCommand);

program
  .command('status')
  .description('Check graph health, staleness, and build provenance (which astria and extraction rules built it)')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--json', 'Emit machine-readable JSON')
  .action(statusCommand);


registerInstallCommand(program);
registerHookCommand(program);

const mergeDriverCmd = program
  .command('merge-driver')
  .description('Git merge driver for .astria/graph.json — union-merges parallel-branch graph commits instead of conflicting');

mergeDriverCmd
  .command('install')
  .description('Wire the driver into .gitattributes + git config (idempotent)')
  .action(() => {
    try {
      for (const msg of mergeDriverInstall(process.cwd())) {
        console.log(msg);
      }
    } catch (err: any) {
      console.error(err.message || err);
      process.exitCode = 1;
    }
  });

mergeDriverCmd
  .command('uninstall')
  .description('Remove the .gitattributes entries and git config the install step added')
  .action(() => {
    try {
      for (const msg of mergeDriverUninstall(process.cwd())) {
        console.log(msg);
      }
    } catch (err: any) {
      console.error(err.message || err);
      process.exitCode = 1;
    }
  });

mergeDriverCmd
  .command('run')
  .description('Invoked by git during a merge — do not call by hand')
  .argument('<base>', 'Base version path (%O)')
  .argument('<ours>', 'Ours version path, merged in place (%A)')
  .argument('<theirs>', 'Theirs version path (%B)')
  .allowUnknownOption(true)
  .action((base: string, ours: string, theirs: string) => {
    process.exitCode = mergeDriverRun(base, ours, theirs);
  });

program
  .command('digest')
  .description('Engineering digest from the graph: overview, health, hubs, communities, LLM spend — cron/CI friendly (hosted tier: app.graphify.com)')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--out <file>', 'Write the markdown digest to a file instead of stdout')
  .option('--json', 'Emit machine-readable JSON')
  .action(digestCommand);

program
  .command('merge-gate')
  .description('CI merge gate: fails when the graph is missing, stale, unhealthy, or the pending diff is too risky (hosted tier: app.graphify.com)')
  .option('--graph <path>', 'Path to project root', '.')
  .option('--max-age-hours <h>', 'Fail when the graph is older than this', '24')
  .option('--min-health <n>', 'Fail when the health score is below this', '60')
  .option('--max-risk <n>', 'Fail when the diff risk score exceeds this', '70')
  .option('--staged', 'Assess the staged diff only (git diff --cached)')
  .option('--base <ref>', 'Score the committed diff base...head (CI/PR mode — a clean checkout has no working-tree diff; e.g. --base origin/main)')
  .option('--head <ref>', 'Head ref of the range (default HEAD; requires --base)')
  .option('--json', 'Emit machine-readable JSON')
  .action(mergeGateCommand);


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
  .description('Aggregate memory outcomes into .astria/reflections/LESSONS.md')
  .option('--graph <path>', 'Path to project root', '.')
  .action(reflectCommand);

const globalCmd = program
  .command('global')
  .description('Cross-repo global graph: merge many repo graphs into one queryable store');

globalCmd
  .command('add')
  .description('Merge a repo graph into the global store (idempotent by tag)')
  .argument('<path>', 'Repo root with a .astria directory')
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
