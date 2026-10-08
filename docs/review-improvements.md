# Query and indexing contracts

Implementation queries select located symbols as starting nodes. Unresolved expressions can still appear as relationship evidence, with compact text labels; typed records retain their exact identities.

`astria query --json` and MCP `query_graph` return the same query result fields: `text`, `nodes`, `edges`, total `nodeCount`/`edgeCount`, `nextCursor`, `graphGeneration`, `graphBuiltAt`, `freshness`, `renderedTokens`, `elapsedMilliseconds`, and `snapshotEstimatedBytes`. MCP exposes these as `structuredContent` and declares an output schema. CLI additionally includes the request parameters.

The budget bounds the exact rendered `text`, including headers and pagination instructions. Structured records and JSON transport overhead are additional. Nodes and edges describe records delivered on that page; totals describe the matching subgraph. An edge may reference a node on another page. Reuse `nextCursor` with the same question and options; restart pagination if `graphGeneration` changes. Source lines are one-based. Unlocated nodes have an empty `sourceFile` and a null `sourceLine`.

Query validation requires a nonempty question of at most 16,384 UTF-8 bytes, BFS or DFS, depth 0–32, budget 1–100,000, and a nonnegative cursor. MCP rejects unknown arguments and invalid argument types rather than substituting defaults.

Freshness compares current local discovery and content hashes with the indexing manifest, including transcript sidecars. It catches added, deleted, and same-size or timestamp-preserving edits. This requires reading the discoverable corpus; `checkMilliseconds` makes that cost visible. `status` additionally compares database, report, and JSON artifact generations. Age is informational and does not determine source freshness. Global graphs have no single local corpus and return null query freshness. Remote document content is outside the local scan; recorded stale external indexes are disclosed separately.

Every indexing run writes `.astria/performance.json` with measured stage durations, source/database sizes, completion state, and whether derived artifacts were reused. Peak resident memory is measured on Linux and is null elsewhere. Query snapshot memory is an estimate of graph payload storage, excluding allocator and auxiliary index overhead.

Unchanged structural builds reuse completed derived artifacts only when source discovery, effective indexing policy, feedback inputs, graph generation, and artifact generations match. Enriched pipelines continue running to allow partial backend work to retry. Automatic synthetic token benchmarks were removed from indexing; benchmarks remain explicit workflows.

Source retrieval quality gates and task evaluation setup are documented in [quality gates](../scripts/bench/quality/GATE.md) and [task evaluation](../scripts/bench/tasks/README.md). Implementing these tools does not establish evaluation outcomes: actual evaluation requires prepared inputs, working Codex authentication/model access, and independent correctness review.
