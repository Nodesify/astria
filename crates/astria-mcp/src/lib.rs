// astria-mcp: MCP (Model Context Protocol) stdio server exposing the
// knowledge graph to AI agents.
//
// Implements the MCP stdio transport (newline-delimited JSON-RPC 2.0)
// directly — no async runtime — so it can run inside the napi cdylib that
// npm distributes (`astria mcp`).

use std::io::{BufRead, Write};

use rusqlite::Connection;
use serde_json::{json, Value};

use astria_core::Result;
pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub const SERVER_NAME: &str = "astria";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

fn tools() -> Value {
    json!([
        {"name": "query_graph", "description": "BFS/DFS traversal of the knowledge graph for a natural-language question. Returns a compact subgraph context.",
         "inputSchema": {"type": "object", "properties": {
            "question": {"type": "string"},
            "mode": {"type": "string", "enum": ["bfs", "dfs"], "default": "bfs"},
            "depth": {"type": "integer", "default": 2},
            "budget": {"type": "integer", "default": 2000},
            "directed": {"type": "boolean", "default": false,
                "description": "Follow edges only in their stored direction (caller -> callee, importer -> module) instead of both ways."},
            "detail": {"type": "string", "enum": ["all", "high"], "default": "all",
                "description": "'high' keeps only EXTRACTED/DECLARED facts, dropping inferred and semantic edges."},
            "cursor": {"type": "integer", "default": 0,
                "description": "Continuation token from a previous truncated result: fetches the next slice of ranked nodes."}},
            "required": ["question"]}},
        {"name": "repo_map", "description": "Aider-style repo map: files ranked by PageRank over the reference graph with top symbols per file. One budgeted blob to orient on a codebase.",
         "inputSchema": {"type": "object", "properties": {
            "budget": {"type": "integer", "default": 2000},
            "detail": {"type": "string", "enum": ["all", "high"], "default": "all"}}}},
        {"name": "explain", "description": "Explain a node: its metadata and up to 20 neighbors with relations and confidence.",
         "inputSchema": {"type": "object", "properties": {"node": {"type": "string"}}, "required": ["node"]}},
        {"name": "get_neighbors", "description": "List a node's neighbors, optionally filtered by relation.",
         "inputSchema": {"type": "object", "properties": {
            "node": {"type": "string"}, "relation": {"type": "string"}}, "required": ["node"]}},
        {"name": "shortest_path", "description": "Shortest path between two nodes, with relations per hop.",
         "inputSchema": {"type": "object", "properties": {
            "source": {"type": "string"}, "target": {"type": "string"},
            "directed": {"type": "boolean", "default": false,
                "description": "Follow edges only in their stored direction."},
            "detail": {"type": "string", "enum": ["all", "high"], "default": "all"}},
            "required": ["source", "target"]}},
        {"name": "affected", "description": "Blast radius: everything impacted by changing a node (reverse reachability over calls/imports/uses).",
         "inputSchema": {"type": "object", "properties": {
            "node": {"type": "string"}, "depth": {"type": "integer", "default": 2},
            "relation": {"type": "string"}}, "required": ["node"]}},
        {"name": "god_nodes", "description": "The highest-degree nodes — what everything connects through.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "list_communities", "description": "All communities with their hub-based labels, sizes, and cohesion.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "graph_stats", "description": "Node/edge/community/file counts for the graph.",
         "inputSchema": {"type": "object", "properties": {}}}
    ])
}

fn text_result(text: String) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": false})
}

fn error_result(message: String) -> Value {
    json!({"content": [{"type": "text", "text": message}], "isError": true})
}

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn bool_arg(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

/// Fidelity tier: "high" keeps only EXTRACTED/DECLARED facts (strength
/// >= 0.9); anything else keeps all facts.
fn min_strength_for(detail: &Option<String>) -> f64 {
    match detail.as_deref().map(|s| s.to_lowercase()).as_deref() {
        Some("high") => 0.9,
        _ => 0.0,
    }
}

/// Community id -> hub label, for human/agent-readable output.
fn community_label_map(db: &Connection) -> std::collections::HashMap<i64, String> {
    let mut stmt = match db.prepare("SELECT id, label FROM communities") {
        Ok(s) => s,
        Err(_) => return Default::default(),
    };
    stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

fn call_tool(db: &Connection, db_path: &str, name: &str, args: &Value) -> Value {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match name {
        "query_graph" => {
            let question = str_arg(args, "question").unwrap_or_default();
            let mode = str_arg(args, "mode").unwrap_or_else(|| "bfs".into());
            let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
            let budget = args.get("budget").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;
            let directed = bool_arg(args, "directed").unwrap_or(false);
            let detail = str_arg(args, "detail");
            let cursor = args.get("cursor").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            astria_query::query_graph(
                db,
                db_path,
                &question,
                &mode,
                depth as usize,
                budget as i64,
                directed,
                min_strength_for(&detail),
                cursor,
                min_strength_for(&detail) >= 0.9,
            )
            .map(|(text, n, e, next)| {
                let mut out = format!("{text}\n\n({n} nodes, {e} edges)");
                if let Some(next) = next {
                    out.push_str(&format!(
                        "\n(continuation: re-run with cursor {next} for the next nodes)"
                    ));
                }
                text_result(out)
            })
        }
        "repo_map" => {
            let budget = args.get("budget").and_then(|v| v.as_u64()).unwrap_or(2000) as i64;
            let detail = str_arg(args, "detail");
            astria_query::repo_map(db, db_path, budget, min_strength_for(&detail))
                .map(|(text, files)| text_result(format!("{text}\n\n({files} files shown)")))
        }
        "explain" => {
            let node = str_arg(args, "node").unwrap_or_default();
            astria_query::explain_with_neighbors(db, db_path, &node).map(|r| {
                let r = match r {
                    Some(r) => r,
                    None => return error_result(format!("node not found: {node}")),
                };
                let mut out = format!(
                    "{} (id: {}, file: {})\n\nConnections ({}):\n",
                    r.label, r.id, r.source_file, r.neighbor_count
                );
                for n in &r.neighbors {
                    out.push_str(&format!(
                        "  --{} [{}]--> {} ({})\n",
                        n.relation, n.confidence, n.neighbor_label, n.neighbor_file
                    ));
                }
                text_result(out)
            })
        }
        "get_neighbors" => {
            let node = str_arg(args, "node").unwrap_or_default();
            let relation = str_arg(args, "relation");
            astria_query::explain_with_neighbors(db, db_path, &node).map(|r| {
                let r = match r {
                    Some(r) => r,
                    None => return error_result(format!("node not found: {node}")),
                };
                let mut out = String::new();
                for n in &r.neighbors {
                    if let Some(rel) = &relation {
                        if &n.relation != rel {
                            continue;
                        }
                    }
                    out.push_str(&format!(
                        "{} [{}] ({})\n",
                        n.neighbor_label, n.relation, n.neighbor_file
                    ));
                }
                if out.is_empty() {
                    out.push_str("(no matching neighbors)");
                }
                text_result(out)
            })
        }
        "shortest_path" => {
            let source = str_arg(args, "source").unwrap_or_default();
            let target = str_arg(args, "target").unwrap_or_default();
            let directed = bool_arg(args, "directed").unwrap_or(false);
            let detail = str_arg(args, "detail");
            astria_query::find_shortest_path(
                db,
                db_path,
                &source,
                &target,
                directed,
                min_strength_for(&detail),
            )
            .map(|(found, hops, text)| {
                text_result(format!("{text}\n\n(found: {found}, hops: {hops})"))
            })
        }
        "affected" => {
            let node = str_arg(args, "node").unwrap_or_default();
            let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
            let relation = str_arg(args, "relation");
            astria_analyze::affected::affected(db, &node, depth, relation.as_deref()).map(|r| {
                // Stored paths are absolute; show them relative to the
                // project root so lines stay short.
                let root = std::path::Path::new(db_path)
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_string_lossy().replace('\\', "/"));
                let rel = |path: &str| -> String {
                    match &root {
                        Some(r) => astria_paths::relative_display(path, r),
                        None => path.to_string(),
                    }
                };
                let mut out = format!(
                    "Blast radius of {} ({}, {} hits):\n",
                    r.seed_label, r.seed, r.total
                );
                let mut last_depth = 0;
                for h in &r.hits {
                    if h.depth != last_depth {
                        last_depth = h.depth;
                        out.push_str(&format!("\ndepth {}:\n", h.depth));
                    }
                    out.push_str(&format!(
                        "  {} [id={}] ({}) via {}\n",
                        h.label,
                        h.id,
                        h.relation,
                        rel(&h.via_file)
                    ));
                }
                text_result(out)
            })
        }
        "god_nodes" => astria_analyze::analyze(db).map(|a| {
            let community_labels = community_label_map(db);
            let mut out = String::from("Top hubs:\n");
            for n in &a.god_nodes {
                // Print the community's hub label, never the raw Option number.
                let comm = match n.community {
                    Some(c) => community_labels
                        .get(&(c as i64))
                        .cloned()
                        .unwrap_or_else(|| c.to_string()),
                    None => "-".to_string(),
                };
                out.push_str(&format!(
                    "  {} (degree {}, community {})\n",
                    n.label, n.degree, comm
                ));
            }
            text_result(out)
        }),
        "list_communities" => {
            let mut stmt =
                db.prepare("SELECT id, label, cohesion, size FROM communities ORDER BY size DESC")?;
            let rows: Vec<(i64, String, Option<f64>, i64)> = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<std::result::Result<_, _>>()?;
            let modularity: Option<f64> = db
                .query_row(
                    "SELECT CAST(value AS REAL) FROM _meta WHERE key = 'last_modularity'",
                    [],
                    |r| r.get(0),
                )
                .ok();
            let modularity_txt = modularity
                .map(|q| format!(" (modularity {q:.3})"))
                .unwrap_or_default();
            let mut out = format!("{} communities{modularity_txt}:\n", rows.len());
            for (id, label, cohesion, size) in rows {
                out.push_str(&format!(
                    "  [{id}] {label} - {size} nodes, cohesion {}\n",
                    cohesion
                        .map(|c| format!("{c:.2}"))
                        .unwrap_or_else(|| "-".into())
                ));
            }
            Ok(text_result(out))
        }
        "graph_stats" => {
            let nodes: i64 = db
                .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
                .unwrap_or(0);
            let edges: i64 = db
                .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
                .unwrap_or(0);
            let communities: i64 = db
                .query_row("SELECT COUNT(*) FROM communities", [], |r| r.get(0))
                .unwrap_or(0);
            let files: i64 = db
                .query_row("SELECT COUNT(*) FROM file_manifest", [], |r| r.get(0))
                .unwrap_or(0);
            let modularity: Option<f64> = db
                .query_row(
                    "SELECT CAST(value AS REAL) FROM _meta WHERE key = 'last_modularity'",
                    [],
                    |r| r.get(0),
                )
                .ok();
            let modularity_txt = modularity
                .map(|q| format!(", modularity: {q:.3}"))
                .unwrap_or_default();
            Ok(text_result(format!(
                "nodes: {nodes}, edges: {edges}, communities: {communities}, files tracked: {files}{modularity_txt}"
            )))
        }
        other => Ok(error_result(format!("unknown tool: {other}"))),
    }));

    match result {
        Ok(Ok(value)) => value,
        Ok(Err(e)) => error_result(e.to_string()),
        Err(_) => error_result("internal error while executing tool".into()),
    }
}

fn handle_message(db: &Connection, db_path: &str, msg: &Value) -> Option<Value> {
    // Notifications (no id) get no response
    let id = msg.get("id").cloned()?;
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(json!({}));

    let response = match method {
        "initialize" => json!({
            "protocolVersion": params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or(PROTOCOL_VERSION),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
            "instructions": "This server exposes a prebuilt knowledge graph of a codebase. Prefer these tools over grepping or browsing files: call repo_map first to orient, query_graph for natural-language questions about architecture or behavior, explain or get_neighbors for a specific symbol, shortest_path to trace how two things connect, and affected before changing a node to see the blast radius. If graph_stats reports 0 nodes, the graph has not been built yet — tell the user to run `astria run <path>`. After code edits, `astria update <path>` refreshes the graph."
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            return Some(
                json!({"jsonrpc": "2.0", "id": id, "result": call_tool(db, db_path, name, &args)}),
            );
        }
        other => {
            return Some(json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": format!("method not found: {other}")}
            }));
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": response}))
}

/// Run the MCP stdio server loop against the graph at `db_path`.
/// Reads newline-delimited JSON-RPC from stdin, writes responses to stdout.
pub fn serve(db_path: &std::path::Path) -> Result<()> {
    let db = astria_core::open_db(db_path)?;
    let db_path_str = db_path.to_string_lossy().to_string();

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break; // stdin closed
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue, // skip malformed lines
        };
        if let Some(response) = handle_message(&db, &db_path_str, &msg) {
            let mut out = serde_json::to_string(&response)?;
            out.push('\n');
            writer.write_all(out.as_bytes())?;
            writer.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(v: Value) -> Value {
        v
    }

    #[test]
    fn initialize_returns_capabilities() {
        let db = astria_core::open_db_in_memory().unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                         "params": {"protocolVersion": "2025-06-18"}});
        let resp = handle_message(&db, ":memory:", &msg(req)).unwrap();
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["serverInfo"]["name"], SERVER_NAME);
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn initialize_includes_usage_instructions() {
        let db = astria_core::open_db_in_memory().unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        let resp = handle_message(&db, ":memory:", &msg(req)).unwrap();
        let instructions = resp["result"]["instructions"].as_str().unwrap_or("");
        assert!(
            instructions.contains("repo_map") && instructions.contains("query_graph"),
            "instructions should steer agents to the graph tools, got: {instructions}"
        );
        assert!(
            instructions.contains("astria run"),
            "instructions should tell agents how to build a missing graph, got: {instructions}"
        );
    }

    #[test]
    fn notifications_get_no_response() {
        let db = astria_core::open_db_in_memory().unwrap();
        let req = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(handle_message(&db, ":memory:", &msg(req)).is_none());
    }

    #[test]
    fn tools_list_has_all_tools() {
        let db = astria_core::open_db_in_memory().unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let resp = handle_message(&db, ":memory:", &msg(req)).unwrap();
        let names: Vec<&str> = resp["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for expected in [
            "query_graph",
            "repo_map",
            "explain",
            "get_neighbors",
            "shortest_path",
            "affected",
            "god_nodes",
            "list_communities",
            "graph_stats",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
    }

    #[test]
    fn unknown_method_is_jsonrpc_error() {
        let db = astria_core::open_db_in_memory().unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 3, "method": "no/such/method"});
        let resp = handle_message(&db, ":memory:", &msg(req)).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn graph_stats_tool_works() {
        let db = astria_core::open_db_in_memory().unwrap();
        db.execute(
            "INSERT INTO nodes (id, label, file_type, source_file) VALUES ('a', 'A', 'code', 'f.rs')",
            [],
        )
        .unwrap();
        let args = json!({});
        let result = call_tool(&db, ":memory:", "graph_stats", &args);
        assert_eq!(result["isError"], false);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("nodes: 1"));
    }

    #[test]
    fn unknown_tool_is_tool_error() {
        let db = astria_core::open_db_in_memory().unwrap();
        let result = call_tool(&db, ":memory:", "bogus", &json!({}));
        assert_eq!(result["isError"], true);
    }
}
