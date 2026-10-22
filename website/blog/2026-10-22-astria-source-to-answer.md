---
slug: astria-source-to-answer
title: "From source files to an agent answer: Astria's evidence pipeline"
authors: [nodesify]
tags: [architecture]
---

An agent asking who calls `save` needs more than a plausible name match. It needs the intended declaration, a source location, the evidence behind each relationship, and an indication of whether the graph still matches the repository. Astria 1.4.0 makes those boundaries explicit across extraction, resolution, storage, retrieval, and MCP delivery.

<!-- truncate -->

## Indexing records evidence

Discovery selects source files and project metadata. Tree-sitter extraction records declarations and syntax relationships; reference resolution then uses supported imports, lexical scope, and receiver-type evidence to select endpoints. Typed locals cannot leak across function boundaries, and conflicting instance-field assignments do not establish a unique receiver.

The resulting `RESOLVED` relationship says that supported source evidence selected an endpoint. It does not establish the target of every runtime call. Interface dispatch, dynamic behavior, and optional semantic enrichment need their own evidence interpretation. The [architecture walkthrough](/docs/explanation/architecture#from-source-files-to-an-answer) follows each stage and documents those limits.

## Publication and query consistency are separate concerns

Validated core facts, the file manifest, and publication metadata commit together in SQLite. Derived passes and exported artifacts follow their own publication steps. If semantic extraction fails, the core publication does not advance. If a later derived stage fails, an earlier successful core commit can remain.

Queries use immutable, generation-keyed snapshots. A changed publication generation invalidates the process-local cache; freshness metadata tells the caller whether source inputs or extraction rules have changed. That is why an agent should inspect freshness rather than assume that a long-running MCP process is current or stale solely because it has stayed open.

## Retrieval must preserve uncertainty

An exact node ID identifies a seed. A repeated label such as `save` can match several declarations; Astria returns candidate IDs and locations instead of choosing one. With no uniquely resolved explicit seed, it returns a miss without keyword fallback.

After seed selection, traversal discovers related nodes, evidence filters constrain relationships, and rendering fits the answer text within a token budget. A continuation cursor signals more context. The CLI and MCP dispatcher reach the same Rust query engine, so switching interfaces does not create a different interpretation of the graph.

## Give the agent a procedure, not just prose

The [AI workflow](/docs/guides/ai-workflow) specifies how to orient, query, disambiguate, check impact, inspect source, and refresh. It also explains why an unresolved stub must not acquire a fabricated source location and why a missing graph path does not prove that no runtime dependency exists.

The site's [llms.txt](https://nodesify.github.io/astria/llms.txt) and [llms-full.txt](https://nodesify.github.io/astria/llms-full.txt) now include the released documentation and blog articles. They identify the documentation version and retain dated context for historical measurements. Individual Markdown pages support selective retrieval. These exports improve access to the explanation; they do not replace source verification or demonstrate a retrieval-quality gain.
