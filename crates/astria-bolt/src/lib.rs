// astria-bolt: a minimal Bolt client for live Neo4j pushes — the sync,
// dependency-free counterpart to the Cypher file export. Handshake +
// HELLO + RUN/PULL over PackStream is the entire surface `neo4j_push`
// needs; no async runtime, no driver dependency (matching how astria-mcp
// hand-rolls stdio JSON-RPC and astria-semantic hand-rolls HTTP).

pub mod frame;
pub mod neo4j_push;
pub mod packstream;

use packstream::{encode_struct, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;

/// Bolt message struct tags (v3+).
mod tag {
    pub const HELLO: u8 = 0x01;
    pub const RUN: u8 = 0x10;
    pub const PULL: u8 = 0x3F;
    pub const SUCCESS: u8 = 0x70;
    pub const RECORD: u8 = 0x71;
    pub const IGNORED: u8 = 0x7E;
    pub const FAILURE: u8 = 0x7F;
}

/// The handshake magic every Bolt client opens with, followed by four
/// proposed versions.
const MAGIC: [u8; 4] = [0x60, 0x60, 0xB0, 0x17];

/// Parse `bolt://[user:pass@]host[:port]` (also accepts `neo4j://`, treated
/// as a direct single-host connection). Default port 7687.
/// Parsed bolt URL: host, port, and optional userinfo credentials.
pub type ParsedUrl = (String, u16, Option<(String, String)>);

pub fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let rest = url
        .strip_prefix("bolt://")
        .or_else(|| url.strip_prefix("bolt+s://"))
        .or_else(|| url.strip_prefix("neo4j://"))
        .ok_or_else(|| format!("not a bolt URL (expected bolt://...): {url}"))?;
    let (userinfo, hostport) = match rest.split_once('@') {
        Some((userinfo, hostport)) => (Some(userinfo.to_string()), hostport),
        None => (None, rest),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((host, port)) => (
            host.to_string(),
            port.parse::<u16>()
                .map_err(|e| format!("bad port in {url}: {e}"))?,
        ),
        None => (hostport.to_string(), 7687),
    };
    if host.is_empty() {
        return Err(format!("missing host in bolt URL: {url}"));
    }
    let credentials = userinfo.map(|info| match info.split_once(':') {
        Some((user, pass)) => (user.to_string(), pass.to_string()),
        None => (info, String::new()),
    });
    Ok((host, port, credentials))
}

#[derive(Debug)]
pub struct BoltClient {
    stream: TcpStream,
}

/// A server refusal with its metadata map rendered readably.
fn failure_message(fields: &[Value]) -> String {
    if let Some(Value::Map(map)) = fields.first() {
        let code = map
            .get("code")
            .and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let message = map
            .get("message")
            .and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();
        if code.is_empty() && message.is_empty() {
            return "Neo4j failure (no metadata)".into();
        }
        return format!("Neo4j failure {code}: {message}");
    }
    "Neo4j failure (unparseable response)".into()
}

impl BoltClient {
    /// Connect, negotiate the protocol version, and authenticate.
    pub fn connect(url: &str, user: &str, pass: &str) -> astria_core::Result<Self> {
        let (host, port, url_credentials) =
            parse_url(url).map_err(astria_core::AstriaError::Graph)?;
        let (principal, credentials) =
            url_credentials.unwrap_or_else(|| (user.to_string(), pass.to_string()));

        let stream = TcpStream::connect((host.as_str(), port)).map_err(|e| {
            astria_core::AstriaError::Graph(format!("cannot reach {host}:{port}: {e}"))
        })?;

        let mut client = BoltClient { stream };
        client.handshake()?;
        client.hello(&principal, &credentials)?;
        Ok(client)
    }

    /// Magic + four proposals; the server answers with the version it
    /// picked. Only the agreed version's major is validated — HELLO/RUN/
    /// PULL wire shapes are identical across Bolt 3/4/5 for this client's
    /// subset.
    fn handshake(&mut self) -> astria_core::Result<()> {
        let mut offer = Vec::with_capacity(20);
        offer.extend_from_slice(&MAGIC);
        // Versions big-endian: 00 00 <major> <minor>. Propose 4.4, 4.1,
        // 3.0, then zero (no more proposals).
        for version in [0x0000_0404u32, 0x0000_0401, 0x0000_0003, 0x0000_0000] {
            offer.extend_from_slice(&version.to_be_bytes());
        }
        self.stream
            .write_all(&offer)
            .map_err(astria_core::AstriaError::Io)?;
        let mut picked = [0u8; 4];
        self.stream
            .read_exact(&mut picked)
            .map_err(astria_core::AstriaError::Io)?;
        let major = picked[2];
        if !(3..=5).contains(&major) {
            return Err(astria_core::AstriaError::Graph(format!(
                "server rejected Bolt handshake (offered 4.4/4.1/3.0, got {:02x?})",
                picked
            )));
        }
        Ok(())
    }

    fn hello(&mut self, principal: &str, credentials: &str) -> astria_core::Result<()> {
        let mut fields: BTreeMap<String, Value> = BTreeMap::new();
        fields.insert(
            "user_agent".into(),
            Value::String(format!("astria/{}", env!("CARGO_PKG_VERSION"))),
        );
        fields.insert("scheme".into(), Value::String("basic".into()));
        fields.insert("principal".into(), Value::String(principal.into()));
        fields.insert("credentials".into(), Value::String(credentials.into()));
        let mut buf = Vec::new();
        encode_struct(tag::HELLO, &[Value::Map(fields)], &mut buf);
        let (response_tag, response_fields) = self.request(&buf)?;
        match response_tag {
            tag::SUCCESS => Ok(()),
            tag::FAILURE => Err(astria_core::AstriaError::Graph(failure_message(
                &response_fields,
            ))),
            other => Err(astria_core::AstriaError::Graph(format!(
                "unexpected HELLO response tag 0x{other:02X}"
            ))),
        }
    }

    /// Run one statement to completion (RUN + PULL, draining any RECORDs).
    /// Used with parameterized UNWIND batches by the push.
    pub fn run(&mut self, query: &str, params: BTreeMap<String, Value>) -> astria_core::Result<()> {
        let extra: BTreeMap<String, Value> = BTreeMap::new();
        let mut run_buf = Vec::new();
        encode_struct(
            tag::RUN,
            &[
                Value::String(query.into()),
                Value::Map(params),
                Value::Map(extra),
            ],
            &mut run_buf,
        );
        let mut pull_buf = Vec::new();
        encode_struct(tag::PULL, &[Value::Integer(-1)], &mut pull_buf);

        // Pipeline both, then read the two summaries in order.
        frame::write_message(&mut self.stream, &run_buf).map_err(astria_core::AstriaError::Io)?;
        frame::write_message(&mut self.stream, &pull_buf).map_err(astria_core::AstriaError::Io)?;

        let (run_tag, run_fields) = self.read_response()?;
        if run_tag == tag::FAILURE {
            return Err(astria_core::AstriaError::Graph(failure_message(
                &run_fields,
            )));
        }
        loop {
            let (response_tag, response_fields) = self.read_response()?;
            match response_tag {
                tag::RECORD => continue, // write queries return no rows worth keeping
                tag::SUCCESS => return Ok(()),
                tag::FAILURE => {
                    return Err(astria_core::AstriaError::Graph(failure_message(
                        &response_fields,
                    )))
                }
                tag::IGNORED => {
                    return Err(astria_core::AstriaError::Graph(
                        "statement ignored (previous failure rolled the transaction back)".into(),
                    ))
                }
                other => {
                    return Err(astria_core::AstriaError::Graph(format!(
                        "unexpected response tag 0x{other:02X}"
                    )))
                }
            }
        }
    }

    /// Send one message and read exactly one response message.
    fn request(&mut self, message: &[u8]) -> astria_core::Result<(u8, Vec<Value>)> {
        frame::write_message(&mut self.stream, message).map_err(astria_core::AstriaError::Io)?;
        self.read_response()
    }

    /// Deframe one message and classify its struct tag + fields.
    fn read_response(&mut self) -> astria_core::Result<(u8, Vec<Value>)> {
        let message =
            frame::read_message(&mut self.stream).map_err(astria_core::AstriaError::Io)?;
        let (response_tag, fields, _) =
            packstream::decode_struct(&message).map_err(astria_core::AstriaError::Graph)?;
        Ok((response_tag, fields))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A minimal Bolt server: validates the handshake, accepts HELLO and
    /// one RUN+PULL pair per pushed statement, always succeeding. Returns
    /// the RUN queries it received so the test can assert on them.
    fn spawn_mock_server() -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();

            // Handshake: 20 bytes in, 4-byte version out.
            let mut handshake = [0u8; 20];
            sock.read_exact(&mut handshake).unwrap();
            assert_eq!(&handshake[0..4], &MAGIC);
            sock.write_all(&0x0000_0404u32.to_be_bytes()).unwrap();

            // HELLO -> SUCCESS.
            let hello = frame::read_message(&mut sock).unwrap();
            let (hello_tag, _, _) = packstream::decode_struct(&hello).unwrap();
            assert_eq!(hello_tag, tag::HELLO);
            let mut success = Vec::new();
            packstream::encode_struct(
                tag::SUCCESS,
                &[Value::Map(Default::default())],
                &mut success,
            );
            frame::write_message(&mut sock, &success).unwrap();

            // Statement pairs until the client hangs up.
            loop {
                // RUN (a missing read is the client closing — end cleanly).
                let message = match frame::read_message(&mut sock) {
                    Ok(m) if !m.is_empty() => m,
                    _ => return,
                };
                let (run_tag, fields, _) = match packstream::decode_struct(&message) {
                    Ok(parsed) => parsed,
                    _ => return,
                };
                if run_tag != tag::RUN {
                    return;
                }
                let query = match fields.first() {
                    Some(Value::String(q)) => q.clone(),
                    _ => return,
                };
                tx.send(query).unwrap();

                let pull = frame::read_message(&mut sock).unwrap();
                let (pull_tag, _, _) = packstream::decode_struct(&pull).unwrap();
                assert_eq!(pull_tag, tag::PULL);

                let mut success = Vec::new();
                packstream::encode_struct(
                    tag::SUCCESS,
                    &[Value::Map(Default::default())],
                    &mut success,
                );
                frame::write_message(&mut sock, &success).unwrap(); // RUN summary
                frame::write_message(&mut sock, &success).unwrap(); // PULL summary
            }
        });
        (format!("bolt://127.0.0.1:{port}"), rx)
    }

    #[test]
    fn url_parsing() {
        assert_eq!(
            parse_url("bolt://localhost:7688").unwrap(),
            ("localhost".into(), 7688, None)
        );
        assert_eq!(
            parse_url("bolt://neo4j:secret@db.example.com").unwrap(),
            (
                "db.example.com".into(),
                7687,
                Some(("neo4j".into(), "secret".into()))
            )
        );
        assert!(parse_url("http://localhost:7687").is_err());
        assert!(parse_url("bolt://").is_err());
    }

    #[test]
    fn connect_run_and_receive_on_mock_server() {
        let (url, rx) = spawn_mock_server();
        let mut client = BoltClient::connect(&url, "neo4j", "password").unwrap();

        let mut params = BTreeMap::new();
        params.insert("rows".into(), Value::List(vec![Value::Integer(1)]));
        client
            .run("UNWIND $rows AS row MERGE (n:Symbol {id: row.id})", params)
            .unwrap();
        client.run("RETURN 1", BTreeMap::new()).unwrap();

        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            "UNWIND $rows AS row MERGE (n:Symbol {id: row.id})"
        );
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            "RETURN 1"
        );
    }

    #[test]
    fn auth_failure_surfaces_server_message() {
        // A server that FAILs HELLO with Neo4j's auth error shape.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut handshake = [0u8; 20];
            sock.read_exact(&mut handshake).unwrap();
            sock.write_all(&0x0000_0404u32.to_be_bytes()).unwrap();
            let _hello = frame::read_message(&mut sock).unwrap();
            let mut map = BTreeMap::new();
            map.insert(
                "code".into(),
                Value::String("Neo4j.Security.Unauthorized".into()),
            );
            map.insert(
                "message".into(),
                Value::String("The client is unauthorized".into()),
            );
            let mut failure = Vec::new();
            packstream::encode_struct(tag::FAILURE, &[Value::Map(map)], &mut failure);
            frame::write_message(&mut sock, &failure).unwrap();
        });

        let err =
            BoltClient::connect(&format!("bolt://127.0.0.1:{port}"), "neo4j", "wrong").unwrap_err();
        let text = err.to_string();
        assert!(text.contains("Unauthorized"), "got: {text}");
        assert!(text.contains("Neo4j.Security"), "got: {text}");
    }

    #[test]
    fn unreachable_server_is_a_clear_error() {
        // Port 1 on loopback: nothing listens there in any sane environment.
        let err = BoltClient::connect("bolt://127.0.0.1:1", "neo4j", "x").unwrap_err();
        assert!(err.to_string().contains("cannot reach"), "got: {err}");
    }
}
