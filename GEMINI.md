
## astria

This project has an astria knowledge graph at .astria/.
Access it through whichever path your agent has:
- MCP (when an astria MCP server is connected): repo_map, query_graph, explain,
  get_neighbors, shortest_path, affected.
- CLI (works everywhere): astria map, query, explain, path, affected.

Always-on behaviors:
1. Prefer the graph over repeated text searches for architecture questions, feature
   location, cross-file logic flow, and change impact; orient with repo_map (or map,
   or .astria/graph_report.md), and run affected <node> before changing a shared symbol.
2. Before running grep/ripgrep to locate code, try astria query first --
   it answers with file:line provenance in one call against the already-built graph.
3. After modifying code, run astria update . (AST-only, no API cost) so the
   graph stays fresh; queries then report accurate staleness metadata.
<!-- astria:managed -->