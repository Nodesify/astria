# Graph Report

**Nodes:** 1708 | **Edges:** 7965 | **Communities:** 212 | **Modularity:** 0.536

_Built by graphify v0.7.0._

## Communities

- **nodesify-graphify** (140 nodes, cohesion 0.59)
- **sample.cpp** (114 nodes, cohesion 0.72)
- **path** (99 nodes, cohesion 0.39)
- **build_payload()** (57 nodes, cohesion 0.40)
- **export_wiki.rs** (51 nodes, cohesion 0.48)
- **config()** (51 nodes, cohesion 0.64)
- **lib.rs** (46 nodes, cohesion 0.23)
- **log** (46 nodes, cohesion 0.57)
- **get()** (41 nodes, cohesion 0.44)
- **graphify skill (OpenCode)** (36 nodes, cohesion 0.85)

... and 202 more communities (see the MCP list_communities tool).

## Key Files

- **crates/graphify-analyze/src/affected.rs** (1300 edge endpoints)
- **crates/graphify-analyze/src/lib.rs** (1004 edge endpoints)
- **crates/graphify-semantic/src/lib.rs** (841 edge endpoints)
- **crates/graphify-query/src/lib.rs** (756 edge endpoints)
- **crates/graphify-napi/src/lib.rs** (659 edge endpoints)
- **crates/graphify-build/src/dedup.rs** (487 edge endpoints)
- **crates/graphify-napi/src/pipeline.rs** (484 edge endpoints)
- **crates/graphify-extract/src/walkers.rs** (448 edge endpoints)
- **crates/graphify-napi/src/export_wiki.rs** (371 edge endpoints)
- **crates/graphify-extract/src/manifest.rs** (364 edge endpoints)

## Hub Nodes (God Nodes)

- **lib.rs** (degree: 305, community: lib.rs)
- **call_tool()** (degree: 88, community: call_tool())
- **installPlatform()** (degree: 85, community: join)
- **assert()** (degree: 76, community: assert())
- **extract_import_module()** (degree: 64, community: trim)
- **build_payload()** (degree: 63, community: build_payload())
- **get()** (degree: 62, community: get())
- **cluster()** (degree: 62, community: get())
- **generate_report()** (degree: 61, community: push_str)
- **RepoMapJs** (degree: 61, community: build_payload())

## Surprising Connections

Top 25 of the cross-community edges, ranked by novelty (bigger, more cohesive communities joined by fewer edges score higher):

- **std::chrono::milliseconds** -> **std::time::systemtime::now** (similar_to) [sample.cpp -> nodesify-graphify] (score: 114.00)
- **lib.rs** -> **invalidate_graph_cache()** (contains) [lib.rs -> sample.cpp] (score: 46.00)
- **printTable()** -> **values** (calls) [log -> get()] (score: 41.00)
- **skill.md** -> **index.md** (similar_to) [graphify skill (OpenCode) -> path] (score: 36.00)
- **php.rs** -> **export_html.rs** (similar_to) [config() -> map_err] (score: 34.00)
- **perm_params()** -> **std::sync::lazylock::new** (calls) [chars -> sample.cpp] (score: 34.00)
- **suggest_questions()** -> **replace** (calls) [vec::new -> log] (score: 33.00)
- **string** -> **stringify** (similar_to) [sample.cpp -> assert()] (score: 31.00)
- **assert()** -> **error** (calls) [assert() -> log] (score: 31.00)
- **LanguageConfig** -> **ExtractConfig** (similar_to) [config() -> export_wiki.rs] (score: 25.50)
- **LanguageConfig** -> **PipelineConfig** (similar_to) [config() -> sample.cpp] (score: 25.50)
- **pub_use_schema** -> **schema.rs** (similar_to) [config() -> export_wiki.rs] (score: 25.50)
- **types.rs** -> **FileType** (contains) [config() -> sample.cpp] (score: 25.50)
- **Sample.java** -> **sample.ts** (similar_to) [sample.cpp -> path] (score: 24.75)
- **sample.js** -> **sample.ts** (similar_to) [sample.cpp -> path] (score: 24.75)
- **sample.py** -> **sample.ts** (similar_to) [sample.cpp -> path] (score: 24.75)
- **sample.rs** -> **sample.ts** (similar_to) [sample.cpp -> path] (score: 24.75)
- **db.rs** -> **ids.rs** (similar_to) [execute_batch -> config()] (score: 24.00)
- **note_name()** -> **trim_matches** (calls) [chars -> walk_structural()] (score: 23.00)
- **load()** -> **db_path()** (similar_to) [vec::new -> join] (score: 23.00)
- **writes_index_community_and_god_node_articles()** -> **hub_labels_written_to_communities_table()** (similar_to) [join -> get()] (score: 23.00)
- **extract_single()** -> **map_err** (calls) [walk_structural() -> map_err] (score: 23.00)
- **walk_structural()** -> **replace** (calls) [walk_structural() -> log] (score: 23.00)
- **node_label()** -> **community_label()** (similar_to) [chars -> ok] (score: 22.00)
- **extract_rationale()** -> **filter_map** (calls) [walk_structural() -> ok] (score: 22.00)

## Merged Duplicates

3 near-duplicate node(s) merged into canonical entities.

## Suggested Questions

- Why does lib.rs have 305 connections — shared core or coupling problem?
- std::chrono::milliseconds similar_to std::time::systemtime::now crosses a community boundary - intentional or accidental coupling?
- What are the responsibilities of the 212 communities?
