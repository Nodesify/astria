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
//! Transport hardening (authentication runs before any body byte is read):
//! bounded concurrent connections, per-line/header-count limits, read/write
//! deadlines, `Content-Length`-only bodies (chunked requests are refused),
//! and DNS-rebinding/Origin defenses required for local HTTP servers by the
//! [MCP transport spec](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports):
//! in loopback mode the `Host` header must itself be loopback, and any
//! present `Origin` must be explicitly allowlisted (native clients send no
//! `Origin` and always pass).
//!
//! No async runtime: one thread per connection (up to the cap), a fresh
//! SQLite handle per request (rusqlite `Connection` is not `Sync`, and
//! per-request open keeps the request path panic-free across threads).

use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use astria_core::AstriaError;
use astria_core::Result;

/// Refuse bodies larger than this before reading them (a malformed or
/// hostile client should not be able to pin server memory).
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;
/// Refuse header lines longer than this (request line included).
const MAX_HEADER_LINE_BYTES: usize = 16 * 1024;
/// Refuse requests with more headers than this.
const MAX_HEADER_COUNT: usize = 100;
/// Hard cap on simultaneously served connections; beyond this the server
/// answers 503 instead of allocating another thread.
const MAX_CONNECTIONS: usize = 32;
/// Socket write deadline: a client that never reads its response cannot
/// hold its thread forever.
const IO_TIMEOUT: Duration = Duration::from_secs(30);
/// Whole-request read deadline: the request line, every header byte, and
/// the body must all arrive within this budget. A per-read timeout alone
/// lets a slowloris drip one byte per timeout and hold a connection slot
/// (and its thread) indefinitely; the shared deadline bounds the TOTAL
/// time a slot can be pinned by slow reading.
const REQUEST_DEADLINE: Duration = Duration::from_secs(30);

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
    /// Browser origins allowed to send requests (`--allow-origin`, matched
    /// exactly, e.g. `http://localhost:5173`). Empty by default: requests
    /// carrying an `Origin` header are refused, while native clients (which
    /// send none) always pass.
    pub allowed_origins: Vec<String>,
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
                return Err(AstriaError::Graph(
                    "empty project name in --projects".into(),
                ));
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
            allowed_origins: Vec::new(),
        })
    }
}

/// Request-line + headers (everything needed to authenticate and bound the
/// body before a single body byte is read).
struct Head {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
}

/// Read the request line and headers with per-line and per-count caps so a
/// hostile client cannot stream an "infinite header" at us. Every
/// non-empty header line counts toward the limit — malformed lines (no
/// `name: value` shape) are rejected, not silently skipped, so they cannot
/// bypass the count either. All reads share one deadline.
fn read_head(
    reader: &mut BufReader<TcpStream>,
    deadline: std::time::Instant,
) -> std::io::Result<Head> {
    let mut line = String::new();
    let n = read_limited_line(reader, &mut line, deadline)?;
    if n == 0 {
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
        if read_limited_line(reader, &mut header, deadline)? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADER_COUNT {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "too many headers",
            ));
        }
        match header.split_once(':') {
            Some((name, value)) if !name.trim().is_empty() => {
                headers.push((name.trim().to_lowercase(), value.trim().to_string()));
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed header line",
                ));
            }
        }
    }
    Ok(Head {
        method,
        path,
        query,
        headers,
    })
}

/// `BufRead::read_line` with a byte cap and a shared deadline: a line that
/// exceeds the limit is an error rather than an unbounded allocation, and
/// each underlying read's timeout is recomputed from the remaining budget
/// so a byte-drip client cannot stretch one line past the deadline.
fn read_limited_line(
    reader: &mut BufReader<TcpStream>,
    out: &mut String,
    deadline: std::time::Instant,
) -> std::io::Result<usize> {
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "request deadline exceeded",
            ));
        }
        reader.get_ref().set_read_timeout(Some(remaining))?;
        let mut byte = [0u8; 1];
        match reader.read(&mut byte)? {
            0 => break,
            _ => {
                bytes.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
                if bytes.len() > MAX_HEADER_LINE_BYTES {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "header line too long",
                    ));
                }
            }
        }
    }
    let n = bytes.len();
    out.push_str(&String::from_utf8_lossy(&bytes));
    Ok(n)
}

fn header<'a>(head: &'a Head, name: &str) -> Option<&'a str> {
    head.headers
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
        Response {
            status,
            content_type: "text/plain",
            body: Vec::new(),
        }
    }
    fn status_text(status: u16) -> &'static str {
        match status {
            200 => "OK",
            202 => "Accepted",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            411 => "Length Required",
            413 => "Payload Too Large",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
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

/// Per-server transport policy: bearer token, loopback mode, and the
/// browser-origin allowlist. Shared with route-level tests.
#[derive(Clone)]
pub(crate) struct TransportPolicy {
    token: Option<String>,
    /// True when the server is bound to a loopback address; Host headers
    /// must then be loopback too (DNS-rebinding defense).
    loopback_only: bool,
    /// Exact-match allowed browser origins (lowercased).
    allowed_origins: Vec<String>,
}

impl TransportPolicy {
    fn from_config(config: &HttpServerConfig, loopback_only: bool) -> Self {
        TransportPolicy {
            token: config.token.clone(),
            loopback_only,
            allowed_origins: config
                .allowed_origins
                .iter()
                .map(|o| o.trim().to_lowercase())
                .filter(|o| !o.is_empty())
                .collect(),
        }
    }
}

/// Resolve which project's graph a request targets. Header wins over query
/// param (headers are what clients configure once); unknown names are a 404,
/// not a silent fallback to the default project.
fn resolve_project<'a>(
    config: &'a HttpServerConfig,
    head: &Head,
) -> std::result::Result<&'a PathBuf, Response> {
    let name = header(head, "x-astria-project")
        .map(|h| h.to_string())
        .or_else(|| query_param(&head.query, "project").map(|p| p.to_string()))
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

/// Constant-time string comparison for token checks (a local attacker with
/// timing visibility should not learn the token byte by byte).
fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn authorized(policy: &TransportPolicy, head: &Head) -> bool {
    match &policy.token {
        None => true,
        Some(expected) => {
            let supplied = header(head, "authorization").unwrap_or("");
            let expected = format!("Bearer {expected}");
            secret_eq(supplied, &expected)
        }
    }
}

/// True when a host name (port stripped, brackets tolerated) is loopback:
/// it parses as a loopback IP address, or it is exactly `localhost`. A
/// `127.`-prefix string check would accept hostnames like
/// `127.attacker.example`, which are attacker-owned DNS names.
fn is_loopback_host_name(name: &str) -> bool {
    let unbracketed = name
        .strip_prefix('[')
        .and_then(|n| n.strip_suffix(']'))
        .unwrap_or(name);
    match unbracketed.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => unbracketed.eq_ignore_ascii_case("localhost"),
    }
}

/// DNS-rebinding defense for loopbound servers: when the server itself is
/// loopback, the `Host` the browser resolved must be a loopback name too.
/// (A rebinding attack points `attacker.example` at 127.0.0.1; the request
/// then arrives with `Host: attacker.example`, which is not loopback.)
fn host_allowed(policy: &TransportPolicy, head: &Head) -> bool {
    if !policy.loopback_only {
        return true;
    }
    let Some(host) = header(head, "host") else {
        // HTTP/1.1 requires Host; absence is malformed, not a pass.
        return false;
    };
    let host = host.trim().to_lowercase();
    // Split off an optional :port (rsplit handles [::1]:8620 correctly).
    let name = match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host.as_str(),
    };
    is_loopback_host_name(name)
}

/// Browser-origin policy: native clients send no `Origin` and always pass.
/// A present `Origin` must be explicitly allowlisted — `null` (sandboxed
/// frames) is never allowed.
fn origin_allowed(policy: &TransportPolicy, head: &Head) -> bool {
    match header(head, "origin") {
        None => true,
        Some(origin) => {
            let origin = origin.trim().to_lowercase();
            origin != "null" && policy.allowed_origins.contains(&origin)
        }
    }
}

/// Route one request. Split from the TCP loop so the routing/auth/transport
/// behavior is testable without sockets; `handle` is the JSON-RPC core.
/// Auth, origin, and host checks all run before any body byte is read.
fn route<F>(
    config: &HttpServerConfig,
    policy: &TransportPolicy,
    head: &Head,
    body: &[u8],
    handle: F,
) -> Response
where
    F: Fn(&std::path::Path, &Value) -> Option<Value>,
{
    if !origin_allowed(policy, head) {
        return Response::json(
            403,
            &json!({"error": "origin not allowed; pass --allow-origin to grant browser access"}),
        );
    }
    if !host_allowed(policy, head) {
        return Response::json(
            403,
            &json!({"error": "host header does not match the loopback binding (possible DNS rebinding)"}),
        );
    }
    if !authorized(policy, head) {
        return Response::json(
            401,
            &json!({"error": "unauthorized: missing or invalid bearer token"}),
        );
    }
    match (head.method.as_str(), head.path.as_str()) {
        ("GET", "/healthz") => Response::json(
            200,
            &json!({"ok": true, "projects": config.projects.len(), "default": config.default_project}),
        ),
        ("POST", "/mcp") | ("POST", "/") => {
            let msg: Value = match serde_json::from_slice(body) {
                Ok(v) => v,
                Err(e) => {
                    return Response::json(
                        400,
                        &json!({"jsonrpc": "2.0", "id": Value::Null,
                                "error": {"code": -32700, "message": format!("parse error: {e}")}}),
                    )
                }
            };
            let db_path = match resolve_project(config, head) {
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

/// Serve one connection: read the head (bounded, under one deadline),
/// authenticate/authorize before touching the body, then read the
/// (bounded) body — still inside the same deadline — and route it.
fn handle_connection(mut stream: TcpStream, state: Arc<ServerState>) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    // Every read of this request shares one budget: head, headers, and body
    // together must arrive within REQUEST_DEADLINE.
    let deadline = std::time::Instant::now() + REQUEST_DEADLINE;

    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let head = match read_head(&mut reader, deadline) {
        Ok(head) => head,
        Err(_) => return,
    };

    // Chunked bodies have no bounded length; this server only speaks
    // Content-Length. Anything else is refused before reading.
    if header(&head, "transfer-encoding").is_some() {
        let _ = write_response(
            &mut stream,
            &Response::json(
                411,
                &json!({"error": "Content-Length required (chunked bodies not supported)"}),
            ),
        );
        return;
    }

    // Origin/Host/token checks run before any body byte is read.
    let early = route_early(&state, &head);
    if let Some(resp) = early {
        let _ = write_response(&mut stream, &resp);
        return;
    }

    let content_length = header(&head, "content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        let _ = write_response(
            &mut stream,
            &Response::json(413, &json!({"error": "request body too large"})),
        );
        return;
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        // The body read stays inside the request deadline.
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() || reader.get_ref().set_read_timeout(Some(remaining)).is_err() {
            return;
        }
        if reader.read_exact(&mut body).is_err() {
            return;
        }
    }

    let resp = route(
        &state.config,
        &state.policy,
        &head,
        &body,
        handle_message_for_db,
    );
    let _ = write_response(&mut stream, &resp);
}

/// The origin/host/auth portion of `route`, factored out so it can reject a
/// request before its body is allocated or read. Returns `Some(response)`
/// when the request must not proceed.
fn route_early(state: &ServerState, head: &Head) -> Option<Response> {
    let denied = |resp: Response| Some(resp);
    if !origin_allowed(&state.policy, head) {
        return denied(Response::json(
            403,
            &json!({"error": "origin not allowed; pass --allow-origin to grant browser access"}),
        ));
    }
    if !host_allowed(&state.policy, head) {
        return denied(Response::json(
            403,
            &json!({"error": "host header does not match the loopback binding (possible DNS rebinding)"}),
        ));
    }
    if !authorized(&state.policy, head) {
        return denied(Response::json(
            401,
            &json!({"error": "unauthorized: missing or invalid bearer token"}),
        ));
    }
    None
}

struct ServerState {
    config: HttpServerConfig,
    policy: TransportPolicy,
    active: AtomicUsize,
}

/// Serve until the process is killed. Blocks the calling thread.
pub fn serve_http(config: HttpServerConfig) -> Result<()> {
    let addr = format!("{}:{}", config.host, config.port);
    // A binding without a token must not be reachable off-machine: the
    // server exposes the whole source graph, so remote access without auth
    // is always a misconfiguration, never a mode. Loopback detection parses
    // the host (a `127.`-prefix string check would misjudge
    // `127.attacker.example`).
    let loopback_only = match config.host.trim().parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => is_loopback_host_name(config.host.trim()),
    };
    if config.token.is_none() && !loopback_only {
        return Err(AstriaError::Graph(
            "serving MCP over HTTP on a non-loopback host requires --token (ASTRIA_MCP_TOKEN): \
             the graph is readable source code"
                .into(),
        ));
    }
    let listener = TcpListener::bind(&addr)
        .map_err(|e| AstriaError::Graph(format!("cannot bind {addr}: {e}")))?;
    eprintln!(
        "[astria] MCP HTTP server on http://{addr}/mcp — projects: {} (default '{}'){}{}",
        config.projects.len(),
        config.default_project,
        if config.token.is_some() {
            ", auth: bearer token"
        } else {
            ", auth: none (local only)"
        },
        if config.allowed_origins.is_empty() {
            String::new()
        } else {
            format!(", allowed origins: {}", config.allowed_origins.join(", "))
        },
    );
    let state = Arc::new(ServerState {
        policy: TransportPolicy::from_config(&config, loopback_only),
        config,
        active: AtomicUsize::new(0),
    });
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        // Bounded concurrency: refuse extra clients instead of exhausting
        // threads/memory.
        let active = state.active.fetch_add(1, Ordering::AcqRel) + 1;
        if active > MAX_CONNECTIONS {
            state.active.fetch_sub(1, Ordering::AcqRel);
            let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
            let _ = write_response(
                &mut stream,
                &Response::json(503, &json!({"error": "server busy"})),
            );
            continue;
        }
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            handle_connection(stream, state.clone());
            state.active.fetch_sub(1, Ordering::AcqRel);
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
            allowed_origins: Vec::new(),
        }
    }

    /// Mirror the real parser: header names are lowercased and the query
    /// string is split off the path before routing.
    fn head(method: &str, path: &str, headers: &[(&str, &str)]) -> Head {
        let (path, query) = match path.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (path.to_string(), String::new()),
        };
        Head {
            method: method.into(),
            path,
            query,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_lowercase(), v.to_string()))
                .collect(),
        }
    }

    fn echo_handle(db_path: &std::path::Path, msg: &Value) -> Option<Value> {
        Some(json!({"echo": msg["method"], "db": db_path.to_string_lossy()}))
    }

    fn policy_from(cfg: &HttpServerConfig, loopback: bool) -> TransportPolicy {
        TransportPolicy::from_config(cfg, loopback)
    }

    const AUTHED: &[(&str, &str)] = &[
        ("Authorization", "Bearer secret"),
        ("Host", "127.0.0.1:8620"),
    ];

    #[test]
    fn healthz_reports_projects() {
        let cfg = config();
        let resp = route(
            &cfg,
            &policy_from(&cfg, true),
            &head("GET", "/healthz", AUTHED),
            b"",
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
        let resp = route(
            &cfg,
            &policy_from(&cfg, true),
            &head("GET", "/healthz", &[("Host", "127.0.0.1:8620")]),
            b"",
            echo_handle,
        );
        // healthz sits behind auth too — a probe without the token cannot
        // enumerate anything, and authorized probes send the header anyway.
        assert_eq!(resp.status, 401);
    }

    #[test]
    fn correct_token_authorizes() {
        let cfg = config();
        let resp = route(
            &cfg,
            &policy_from(&cfg, true),
            &head("GET", "/healthz", AUTHED),
            b"",
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
            &policy_from(&cfg, true),
            &head("POST", "/mcp", AUTHED),
            body.as_bytes(),
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
        let mut headers = AUTHED.to_vec();
        headers.push(("x-astria-project", "nope"));
        let resp = route(
            &cfg,
            &policy_from(&cfg, true),
            &head("POST", "/mcp", &headers),
            body.as_bytes(),
            echo_handle,
        );
        assert_eq!(
            resp.status, 404,
            "unknown project names must 404, not fall back"
        );
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
            &policy_from(&cfg, true),
            &head("POST", "/mcp?project=other", &[("Host", "127.0.0.1:8620")]),
            body.as_bytes(),
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
        let resp = route(
            &cfg,
            &policy_from(&cfg, true),
            &head("POST", "/mcp", &[("Host", "127.0.0.1:8620")]),
            b"not json",
            echo_handle,
        );
        assert_eq!(resp.status, 400);
        let out: Value = serde_json::from_slice(&resp.body).unwrap();
        assert_eq!(out["error"]["code"], -32700);
    }

    #[test]
    fn get_mcp_is_method_not_allowed() {
        let mut cfg = config();
        cfg.token = None;
        let resp = route(
            &cfg,
            &policy_from(&cfg, true),
            &head("GET", "/mcp", &[("Host", "127.0.0.1:8620")]),
            b"",
            echo_handle,
        );
        assert_eq!(resp.status, 405);
    }

    #[test]
    fn unknown_path_404s() {
        let mut cfg = config();
        cfg.token = None;
        let resp = route(
            &cfg,
            &policy_from(&cfg, true),
            &head("GET", "/nope", &[("Host", "127.0.0.1:8620")]),
            b"",
            echo_handle,
        );
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn loopback_mode_rejects_non_loopback_host_headers() {
        let cfg = config();
        let policy = policy_from(&cfg, true);
        // A rebound DNS name points at 127.0.0.1 but carries its own Host.
        let mut attacker = AUTHED.to_vec();
        attacker[1] = ("Host", "attacker.example:8620");
        let resp = route(
            &cfg,
            &policy,
            &head("GET", "/healthz", &attacker),
            b"",
            echo_handle,
        );
        assert_eq!(resp.status, 403);

        // Loopback names (with any port) pass; localhost casing is ignored.
        for host in [
            "127.0.0.1",
            "127.0.0.1:8620",
            "LOCALhost:8620",
            "[::1]:8620",
        ] {
            let mut headers = AUTHED.to_vec();
            headers[1] = ("Host", host);
            let resp = route(
                &cfg,
                &policy,
                &head("GET", "/healthz", &headers),
                b"",
                echo_handle,
            );
            assert_eq!(resp.status, 200, "host {host} should be allowed");
        }

        // Missing Host is malformed in HTTP/1.1 and treated as a rejection.
        let resp = route(
            &cfg,
            &policy,
            &head("GET", "/healthz", &[("Authorization", "Bearer secret")]),
            b"",
            echo_handle,
        );
        assert_eq!(resp.status, 403);
    }

    #[test]
    fn remote_mode_skips_host_check_but_keeps_token() {
        let cfg = config();
        let policy = policy_from(&cfg, false);
        // Non-loopback servers are authenticated by token; a proxy may
        // legitimately present an external Host.
        let resp = route(
            &cfg,
            &policy,
            &head(
                "GET",
                "/healthz",
                &[
                    ("Authorization", "Bearer secret"),
                    ("Host", "astria.internal:8620"),
                ],
            ),
            b"",
            echo_handle,
        );
        assert_eq!(resp.status, 200);
        // Without the token it still fails.
        let resp = route(
            &cfg,
            &policy,
            &head("GET", "/healthz", &[("Host", "astria.internal:8620")]),
            b"",
            echo_handle,
        );
        assert_eq!(resp.status, 401);
    }

    #[test]
    fn browser_origins_need_explicit_allowlisting() {
        let cfg = config();
        let policy = policy_from(&cfg, true);
        // Unknown origin, and the sandbox "null" origin: both refused.
        for origin in ["http://evil.example", "null"] {
            let mut headers = AUTHED.to_vec();
            headers.push(("Origin", origin));
            let resp = route(
                &cfg,
                &policy,
                &head("GET", "/healthz", &headers),
                b"",
                echo_handle,
            );
            assert_eq!(resp.status, 403, "origin {origin} must be refused");
        }
        // Native clients send no Origin at all and always pass.
        let resp = route(
            &cfg,
            &policy,
            &head("GET", "/healthz", AUTHED),
            b"",
            echo_handle,
        );
        assert_eq!(resp.status, 200);

        // An explicitly allowlisted origin passes (case-insensitive).
        let mut cfg = cfg;
        cfg.allowed_origins = vec!["http://localhost:5173".into()];
        let policy = policy_from(&cfg, true);
        let mut headers = AUTHED.to_vec();
        headers.push(("Origin", "HTTP://LOCALHOST:5173"));
        let resp = route(
            &cfg,
            &policy,
            &head("GET", "/healthz", &headers),
            b"",
            echo_handle,
        );
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn origin_and_host_are_checked_before_auth() {
        // A rebound host must be rejected even alongside a valid token and
        // an allowlisted origin — none of the three checks is a bypass for
        // another.
        let mut cfg = config();
        cfg.allowed_origins = vec!["http://localhost:5173".into()];
        let policy = policy_from(&cfg, true);
        let mut headers = AUTHED.to_vec();
        headers[1] = ("Host", "rebind.attacker.example");
        headers.push(("Origin", "http://localhost:5173"));
        let resp = route(
            &cfg,
            &policy,
            &head("GET", "/healthz", &headers),
            b"",
            echo_handle,
        );
        assert_eq!(resp.status, 403);
    }

    #[test]
    fn loopback_host_check_parses_ips_not_prefixes() {
        // `127.`-prefix names are attacker DNS names, not loopback.
        for host in [
            "127.attacker.example",
            "127.0.0.1.attacker.example",
            "127.1",
            "10.0.0.1",
            "192.168.1.1",
            "::ffff:127.0.0.1",
            "[::ffff:127.0.0.1]:8620",
        ] {
            assert!(
                !is_loopback_host_name(host),
                "{host} must not count as loopback"
            );
        }
        for host in [
            "127.0.0.1",
            "127.0.0.99",
            "localhost",
            "LOCALHOST",
            "::1",
            "[::1]",
        ] {
            assert!(is_loopback_host_name(host), "{host} must be loopback");
        }
    }

    #[test]
    fn rebinding_prefix_host_is_rejected_at_route_level() {
        let cfg = config();
        let policy = policy_from(&cfg, true);
        let mut attacker = AUTHED.to_vec();
        attacker[1] = ("Host", "127.attacker.example:8620");
        let resp = route(
            &cfg,
            &policy,
            &head("GET", "/healthz", &attacker),
            b"",
            echo_handle,
        );
        assert_eq!(resp.status, 403, "127.<attacker-domain> is not loopback");
    }

    #[test]
    fn secret_eq_is_length_safe_and_accurate() {
        assert!(secret_eq("Bearer abc", "Bearer abc"));
        assert!(!secret_eq("Bearer abc", "Bearer abd"));
        assert!(!secret_eq("Bearer abc", "Bearer abcd"));
        assert!(!secret_eq("", "x"));
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
        let cfg = HttpServerConfig::from_roots(&project, &[format!("alias={}", second.display())])
            .unwrap();
        assert_eq!(cfg.default_project, "my-repo");
        assert!(cfg.projects.contains_key("alias"));
        assert!(cfg.projects.contains_key("my-repo"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
