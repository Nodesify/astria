//! MCP Streamable-HTTP transport with multi-project serving.
//!
//! The stdio transport (lib.rs) serves exactly one graph per process; this
//! module serves many graphs from one process over TCP so teams and CI can
//! point several clients at a single `astria mcp --http` server:
//!
//! - `POST /mcp` with a JSON-RPC body → one JSON-RPC response (JSON responses
//!   are a spec-compliant Streamable HTTP mode; no SSE stream is needed for
//!   the tool server's request/response shape).
//! - `GET /healthz` → liveness probe for process managers.
//! - Project selection per request: `x-astria-project` header, then
//!   `?project=` query parameter, then the server's default project.
//! - Optional bearer auth (`--token`) — required automatically when the
//!   server binds a non-loopback host.
//!
//! No async runtime: one thread per connection, a fresh SQLite handle per
//! request (rusqlite `Connection` is not `Sync`, and per-request open keeps
//! the request path panic-free across threads).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

use serde_json::{json, Value};

use astria_core::AstriaError;
use astria_core::Result;

/// Refuse bodies larger than this before reading them (a malformed or
/// hostile client should not be able to pin server memory).
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

/// One served graph: project name -> path to the graph's `.astria/db.sqlite`.
pub struct HttpServerConfig {
    pub host: String,
    pub port: u16,
    /// When set, every request must carry `Authorization: Bearer <token>`.
    pub token: Option<String>,
    /// project name -> db.sqlite path. The default project must be a key.
    pub projects: HashMap<String, PathBuf>,
    /// Project served when the request names none.
    pub default_project: String,
}

impl HttpServerConfig {
    /// Build the config from a default root plus `name=path` / `path`
    /// entries. Project names default to the directory's file name.
    pub fn from_roots(root: &std::path::Path, extra: &[String]) -> Result<Self> {
        let mut projects: HashMap<String, PathBuf> = HashMap::new();
        let db_for = |root: &std::path::Path| -> Result<PathBuf> {
            let db = root.join(".astria").join("db.sqlite");
            if !db.exists() {
                return Err(AstriaError::Graph(format!(
                    "no graph at {} — run `astria run {}` first",
                    db.display(),
                    root.display()
                )));
            }
            Ok(db)
        };

        let default_db = db_for(root)?;
        let default_project = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string());
        projects.insert(default_project.clone(), default_db);

        for entry in extra {
            let (name, path) = match entry.split_once('=') {
                Some((name, path)) => (name.trim().to_string(), PathBuf::from(path.trim())),
                None => {
                    let path = PathBuf::from(entry.trim());
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| entry.clone());
                    (name, path)
                }
            };
            if name.is_empty() {
                return Err(AstriaError::Graph("empty project name in --projects".into()));
            }
            let db = db_for(&path)?;
            projects.insert(name, db);
        }

        Ok(Self {
            host: "127.0.0.1".to_string(),
            port: 8620,
            token: None,
            projects,
            default_project,
        })
    }
}

/// One parsed HTTP request (only what the routes need).
struct Request {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

enum Body {
    Full,
    TooLarge,
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<(Request, Body)> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "client closed",
        ));
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_uppercase();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };

    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_lowercase(), value.trim().to_string()));
        }
    }

    let content_length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        return Ok((
            Request { method, path, query, headers, body: Vec::new() },
            Body::TooLarge,
        ));
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    Ok((Request { method, path, query, headers, body }, Body::Full))
}

fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// Query params, tolerating both `a=1&b=2` and raw values.
fn query_param<'q>(query: &'q str, name: &str) -> Option<&'q str> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then_some(v)
    })
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn json(status: u16, value: &Value) -> Self {
        Response {
            status,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec()),
        }
    }
    fn empty(status: u16) -> Self {
        Response { status, content_type: "text/plain", body: Vec::new() }
    }
    fn status_text(status: u16) -> &'static str {
        match status {
            200 => "OK",
            202 => "Accepted",
            400 => "Bad Request",
            401 => "Unauthorized",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Payload Too Large",
            500 => "Internal Server Error",
            _ => "OK",
        }
    }
}

fn write_response(stream: &mut TcpStream, resp: &Response) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        resp.status,
        Response::status_text(resp.status),
        resp.content_type,
        resp.body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&resp.body)?;
    stream.flush()
}

/// Resolve which project's graph a request targets. Header wins over query
/// param (headers are what clients configure once); unknown names are a 404,
/// not a silent fallback to the default project.
fn resolve_project<'a>(
    config: &'a HttpServerConfig,
    req: &Request,
) -> std::result::Result<&'a PathBuf, Response> {
    let name = header(req, "x-astria-project")
        .map(|h| h.to_string())
        .or_else(|| query_param(&req.query, "project").map(|p| p.to_string()))
        .unwrap_or_else(|| config.default_project.clone());
    config.projects.get(&name).ok_or_else(|| {
        Response::json(
            404,
            &json!({
                "error": "unknown project",
                "project": name,
                "available": config.projects.keys().cloned().collect::<Vec<_>>(),
            }),
        )
    })
}

fn authorized(config: &HttpServerConfig, req: &Request) -> bool {
    match &config.token {
        None => true,
        Some(expected) => {
            let supplied = header(req, "authorization").unwrap_or("");
            supplied == format!("Bearer {expected}")
        }
    }
}

/// Route one request. Split from the TCP loop so the routing/auth/transport
/// behavior is testable without sockets; `handle` is the JSON-RPC core.
fn route<F>(config: &HttpServerConfig, req: &Request, handle: F) -> Response
where
    F: Fn(&std::path::Path, &Value) -> Option<Value>,
{
    if !authorized(config, req) {
        return Response::json(401, &json!({"error": "unauthorized: missing or invalid bearer token"}));
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/healthz") => Response::json(
            200,
            &json!({"ok": true, "projects": config.projects.len(), "default": config.default_project}),
        ),
        ("POST", "/mcp") | ("POST", "/") => {
            let msg: Value = match serde_json::from_slice(&req.body) {
                Ok(v) => v,
                Err(e) => {
                    return Response::json(
                        400,
                        &json!({"jsonrpc": "2.0", "id": Value::Null,
                                "error": {"code": -32700, "message": format!("parse error: {e}")}}),
                    )
                }
            };
            let db_path = match resolve_project(config, req) {
                Ok(p) => p,
                Err(resp) => return resp,
            };
            // Notifications get no body — 202 per the Streamable HTTP spec.
            match handle(db_path, &msg) {
                Some(response) => Response::json(200, &response),
                None => Response::empty(202),
            }
        }
        ("GET", "/mcp") => Response::json(
            405,
            &json!({"error": "SSE streaming is not supported; POST JSON-RPC to /mcp"}),
        ),
        _ => Response::json(404, &json!({"error": "not found"})),
    }
}

/// Open the project's graph and run one JSON-RPC message through the shared
/// core. A fresh connection per request: `Connection` is not `Sync`, and a
/// read-only tool server pays nothing meaningful for the reopen.
fn handle_message_for_db(db_path: &std::path::Path, msg: &Value) -> Option<Value> {
    let db = match astria_core::open_db(db_path) {
        Ok(db) => db,
        Err(e) => {
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            return Some(json!({
                "jsonrpc": "2.0", "id": id,
                "error": {"code": -32603, "message": format!("cannot open graph: {e}")}
            }));
        }
    };
    let db_path_str = db_path.to_string_lossy().to_string();
    crate::handle_message(&db, &db_path_str, msg)
}

/// Serve until the process is killed. Blocks the calling thread.
pub fn serve_http(config: HttpServerConfig) -> Result<()> {
    let addr = format!("{}:{}", config.host, config.port);
    // A binding without a token must not be reachable off-machine: the
    // server exposes the whole source graph, so remote access without auth
    // is always a misconfiguration, never a mode.
    if config.token.is_none() && !addr.starts_with("127.") && !addr.starts_with("[::1]") {
        return Err(AstriaError::Graph(
            "serving MCP over HTTP on a non-loopback host requires --token (ASTRIA_MCP_TOKEN): \
             the graph is readable source code"
                .into(),
        ));
    }
    let listener = TcpListener::bind(&addr)
        .map_err(|e| AstriaError::Graph(format!("cannot bind {addr}: {e}")))?;
    eprintln!(
        "[astria] MCP HTTP server on http://{addr}/mcp — projects: {} (default '{}'){}",
        config.projects.len(),
        config.default_project,
        if config.token.is_some() { ", auth: bearer token" } else { ", auth: none (local only)" }
    );
    let config = std::sync::Arc::new(config);
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        let config = std::sync::Arc::clone(&config);
        std::thread::spawn(move || {
            let _ = stream.set_nodelay(true);
            let (req, body_state) = match read_request(&mut stream) {
                Ok(pair) => pair,
                Err(_) => return,
            };
            if matches!(body_state, Body::TooLarge) {
                let _ = write_response(
                    &mut stream,
                    &Response::json(413, &json!({"error": "request body too large"})),
                );
                return;
            }
            let resp = route(&config, &req, handle_message_for_db);
            let _ = write_response(&mut stream, &resp);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> HttpServerConfig {
        let mut projects = HashMap::new();
        projects.insert("app".to_string(), PathBuf::from(":memory:"));
        HttpServerConfig {
            host: "127.0.0.1".into(),
            port: 0,
            token: Some("secret".into()),
            projects,
            default_project: "app".into(),
        }
    }

    fn req(method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Request {
        // Mirror the real parser: header names are lowercased and the query
        // string is split off the path before routing.
        let (path, query) = match path.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (path.to_string(), String::new()),
        };
        Request {
            method: method.into(),
            path,
            query,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_lowercase(), v.to_string()))
                .collect(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn echo_handle(db_path: &std::path::Path, msg: &Value) -> Option<Value> {
        Some(json!({"echo": msg["method"], "db": db_path.to_string_lossy()}))
    }

    #[test]
    fn healthz_reports_projects() {
        let cfg = config();
        let resp = route(
            &cfg,
            &req("GET", "/healthz", &[("Authorization", "Bearer secret")], ""),
            echo_handle,
        );
        assert_eq!(resp.status, 200);
        let out: Value = serde_json::from_slice(&resp.body).unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["projects"], 1);
        assert_eq!(out["default"], "app");
    }

    #[test]
    fn missing_token_is_unauthorized() {
        let cfg = config();
        let resp = route(&cfg, &req("GET", "/healthz", &[], ""), echo_handle);
        // healthz sits behind auth too — a probe without the token cannot
        // enumerate anything, and authorized probes send the header anyway.
        assert_eq!(resp.status, 401);
    }

    #[test]
    fn correct_token_authorizes() {
        let cfg = config();
        let resp = route(
            &cfg,
            &req("GET", "/healthz", &[("Authorization", "Bearer secret")], ""),
            echo_handle,
        );
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn post_mcp_routes_to_default_project() {
        let cfg = config();
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
        let resp = route(
            &cfg,
            &req("POST", "/mcp", &[("Authorization", "Bearer secret")], &body),
            echo_handle,
        );
        assert_eq!(resp.status, 200);
        let out: Value = serde_json::from_slice(&resp.body).unwrap();
        assert_eq!(out["echo"], "tools/list");
    }

    #[test]
    fn header_selects_project() {
        let cfg = config();
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": "x"}).to_string();
        let resp = route(
            &cfg,
            &req(
                "POST",
                "/mcp",
                &[("Authorization", "Bearer secret"), ("x-astria-project", "nope")],
                &body,
            ),
            echo_handle,
        );
        assert_eq!(resp.status, 404, "unknown project names must 404, not fall back");
    }

    #[test]
    fn query_param_selects_project() {
        let mut cfg = config();
        cfg.token = None;
        let mut projects = HashMap::new();
        projects.insert("other".to_string(), PathBuf::from("other.sqlite"));
        projects.insert("app".to_string(), PathBuf::from("app.sqlite"));
        cfg.projects = projects;
        cfg.default_project = "app".into();

        let body = json!({"jsonrpc": "2.0", "id": 1, "method": "x"}).to_string();
        let resp = route(
            &cfg,
            &req("POST", "/mcp?project=other", &[], &body),
            |db, msg| Some(json!({"db": db.to_string_lossy(), "m": msg["method"]})),
        );
        assert_eq!(resp.status, 200);
        let out: Value = serde_json::from_slice(&resp.body).unwrap();
        assert_eq!(out["db"], "other.sqlite");
    }

    #[test]
    fn malformed_json_is_parse_error() {
        let mut cfg = config();
        cfg.token = None;
        let resp = route(&cfg, &req("POST", "/mcp", &[], "not json"), echo_handle);
        assert_eq!(resp.status, 400);
        let out: Value = serde_json::from_slice(&resp.body).unwrap();
        assert_eq!(out["error"]["code"], -32700);
    }

    #[test]
    fn get_mcp_is_method_not_allowed() {
        let mut cfg = config();
        cfg.token = None;
        let resp = route(&cfg, &req("GET", "/mcp", &[], ""), echo_handle);
        assert_eq!(resp.status, 405);
    }

    #[test]
    fn unknown_path_404s() {
        let mut cfg = config();
        cfg.token = None;
        let resp = route(&cfg, &req("GET", "/nope", &[], ""), echo_handle);
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn from_roots_names_projects_after_directories() {
        let dir = std::env::temp_dir().join(format!("astria-mcp-test-{}", std::process::id()));
        let project = dir.join("my-repo");
        std::fs::create_dir_all(project.join(".astria")).unwrap();
        std::fs::write(project.join(".astria").join("db.sqlite"), b"").unwrap();

        let cfg = HttpServerConfig::from_roots(
            &project,
            &[format!("alias={}", dir.join("second").display())],
        );
        // The alias target has no graph — the config builder must fail loudly
        // rather than serve a project that 500s on every call.
        assert!(cfg.is_err());

        let second = dir.join("second");
        std::fs::create_dir_all(second.join(".astria")).unwrap();
        std::fs::write(second.join(".astria").join("db.sqlite"), b"").unwrap();
        let cfg = HttpServerConfig::from_roots(
            &project,
            &[format!("alias={}", second.display())],
        )
        .unwrap();
        assert_eq!(cfg.default_project, "my-repo");
        assert!(cfg.projects.contains_key("alias"));
        assert!(cfg.projects.contains_key("my-repo"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
