// astria-bolt: a minimal Bolt client for live Neo4j pushes — the sync,
// dependency-light counterpart to the Cypher file export. Handshake +
// HELLO + RUN/PULL over PackStream is the entire surface `neo4j_push`
// needs; no async runtime, no driver dependency (matching how astria-mcp
// hand-rolls stdio JSON-RPC and astria-semantic hand-rolls HTTP). TLS
// schemes (`+s`, `+ssc`) ride rustls, which the workspace already builds
// via ureq.

pub mod frame;
pub mod neo4j_push;
pub mod packstream;

use packstream::{encode_struct, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Bolt message struct tags (v3+).
mod tag {
    pub const HELLO: u8 = 0x01;
    pub const RUN: u8 = 0x10;
    /// PULL in Bolt 4+ (one `extra` field); PULL_ALL in Bolt 3 (fieldless).
    pub const PULL: u8 = 0x3F;
    pub const SUCCESS: u8 = 0x70;
    pub const RECORD: u8 = 0x71;
    pub const IGNORED: u8 = 0x7E;
    pub const FAILURE: u8 = 0x7F;
}

/// The handshake magic every Bolt client opens with, followed by four
/// proposed versions.
const MAGIC: [u8; 4] = [0x60, 0x60, 0xB0, 0x17];

/// Read/write deadline on the negotiated transport so a silent server or
/// half-open peer cannot pin the calling thread forever.
const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// Transport negotiated from the URL scheme. Neo4j defines `+s` as TLS with
/// certificate verification and `+ssc` as TLS without it (self-signed
/// certificates). A secure scheme must never downgrade to plaintext, and a
/// plaintext URL never silently gains encryption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    /// `bolt://` / `neo4j://` — plaintext TCP.
    Plain,
    /// `bolt+s://` / `neo4j+s://` — TLS with webpki roots and hostname
    /// verification.
    TlsVerified,
    /// `bolt+ssc://` / `neo4j+ssc://` — TLS without certificate verification
    /// (explicitly insecure, for self-signed deployments).
    TlsUnverified,
}

/// Parsed bolt URL: transport scheme, host, port, optional userinfo
/// credentials.
pub struct ParsedUrl {
    pub scheme: Scheme,
    pub host: String,
    pub port: u16,
    pub credentials: Option<(String, String)>,
}

/// Encode a protocol version in the handshake's wire layout:
/// `00 00 <minor> <major>` — Bolt 4.1 is `00 00 01 04`. The major version
/// is the LOW byte; `0x00000401` on the wire means 1.4, not 4.1.
/// [Handshake spec](https://neo4j.com/docs/bolt/current/bolt/handshake/)
fn encode_version(major: u8, minor: u8) -> u32 {
    ((minor as u32) << 8) | major as u32
}

pub fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let (scheme, rest) = [
        ("bolt://", Scheme::Plain),
        ("neo4j://", Scheme::Plain),
        ("bolt+s://", Scheme::TlsVerified),
        ("neo4j+s://", Scheme::TlsVerified),
        ("bolt+ssc://", Scheme::TlsUnverified),
        ("neo4j+ssc://", Scheme::TlsUnverified),
    ]
    .into_iter()
    .find_map(|(prefix, scheme)| url.strip_prefix(prefix).map(|rest| (scheme, rest)))
    .ok_or_else(|| {
        format!("not a bolt URL (expected bolt://, bolt+s:// or bolt+ssc://...): {url}")
    })?;
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
    Ok(ParsedUrl {
        scheme,
        host,
        port,
        credentials,
    })
}

/// Strip IPv6 bracket syntax for socket resolution (`[::1]` → `::1`).
fn connect_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host)
}

/// Duplex byte transport (plain TCP or TLS-over-TCP) the client runs on.
trait ReadWrite: Read + Write + Send {}
impl<T: Read + Write + Send> ReadWrite for T {}

pub struct BoltClient {
    stream: Box<dyn ReadWrite>,
    /// Negotiated (major, minor) protocol version. RUN and PULL wire shapes
    /// differ between Bolt 3 and Bolt 4+, so encoding must follow whatever
    /// the server actually picked.
    version: (u8, u8),
}

impl std::fmt::Debug for BoltClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoltClient")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
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
    /// Connect, negotiate the protocol version, and authenticate. The URL
    /// scheme selects plaintext or verified/unverified TLS.
    pub fn connect(url: &str, user: &str, pass: &str) -> astria_core::Result<Self> {
        let parsed = parse_url(url).map_err(astria_core::AstriaError::Graph)?;
        let (principal, credentials) = parsed
            .credentials
            .unwrap_or_else(|| (user.to_string(), pass.to_string()));

        let tcp = TcpStream::connect((connect_host(&parsed.host), parsed.port)).map_err(|e| {
            astria_core::AstriaError::Graph(format!(
                "cannot reach {}:{}: {e}",
                parsed.host, parsed.port
            ))
        })?;
        let _ = tcp.set_read_timeout(Some(IO_TIMEOUT));
        let _ = tcp.set_write_timeout(Some(IO_TIMEOUT));
        let _ = tcp.set_nodelay(true);

        let stream: Box<dyn ReadWrite> = match parsed.scheme {
            Scheme::Plain => Box::new(tcp),
            Scheme::TlsVerified => Box::new(tls::wrap(tcp, &parsed.host, true)?),
            Scheme::TlsUnverified => Box::new(tls::wrap(tcp, &parsed.host, false)?),
        };

        let mut client = BoltClient {
            stream,
            version: (0, 0),
        };
        client.version = client.handshake()?;
        client.hello(&principal, &credentials)?;
        Ok(client)
    }

    /// Magic + four proposals; the server answers with the version it
    /// picked from them. Offered versions are exactly the ones this client
    /// can speak: 5.0 and all 4.x share the RUN/PULL shapes used here, and
    /// 3.0 has its own. Anything else the server replies (including the
    /// all-zero rejection) is an error, never a guess.
    fn handshake(&mut self) -> astria_core::Result<(u8, u8)> {
        const OFFERED: [(u8, u8); 4] = [(5, 0), (4, 4), (4, 1), (3, 0)];
        let mut offer = Vec::with_capacity(20);
        offer.extend_from_slice(&MAGIC);
        for (major, minor) in OFFERED {
            offer.extend_from_slice(&encode_version(major, minor).to_be_bytes());
        }
        self.stream
            .write_all(&offer)
            .map_err(astria_core::AstriaError::Io)?;
        let mut picked = [0u8; 4];
        self.stream
            .read_exact(&mut picked)
            .map_err(astria_core::AstriaError::Io)?;
        if picked == [0, 0, 0, 0] {
            return Err(astria_core::AstriaError::Graph(
                "server rejected all offered Bolt versions (5.0/4.4/4.1/3.0)".into(),
            ));
        }
        // Reply layout mirrors the offer: `00 00 <minor> <major>`.
        let version = (picked[3], picked[2]);
        if !OFFERED.contains(&version) {
            return Err(astria_core::AstriaError::Graph(format!(
                "server selected Bolt {}.{}, which this client cannot speak \
                 (offered 5.0/4.4/4.1/3.0, wire reply {:02X?})",
                version.0, version.1, picked
            )));
        }
        Ok(version)
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
    ///
    /// The message shapes are version-gated:
    /// - Bolt 3: `RUN (query, params, extra)` + fieldless `PULL_ALL`. The
    ///   third `extra` dictionary was ADDED in v3 per the message spec — a
    ///   two-field RUN is the v1/v2 shape, which v3 servers reject.
    /// - Bolt 4/5: identical `RUN (query, params, extra)` + `PULL {n: -1}`
    ///   ([message spec](https://neo4j.com/docs/bolt/current/bolt/message/)).
    pub fn run(&mut self, query: &str, params: BTreeMap<String, Value>) -> astria_core::Result<()> {
        let mut run_buf = Vec::new();
        let mut pull_buf = Vec::new();
        // RUN carries the (possibly empty) extra dictionary in every version
        // this client speaks (3, 4, 5).
        encode_struct(
            tag::RUN,
            &[
                Value::String(query.into()),
                Value::Map(params),
                Value::Map(BTreeMap::new()),
            ],
            &mut run_buf,
        );
        match self.version.0 {
            3 => {
                encode_struct(tag::PULL, &[], &mut pull_buf);
            }
            _ => {
                let mut pull_extra: BTreeMap<String, Value> = BTreeMap::new();
                pull_extra.insert("n".into(), Value::Integer(-1));
                encode_struct(tag::PULL, &[Value::Map(pull_extra)], &mut pull_buf);
            }
        }

        // Pipeline both, then read the two summary messages in order.
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

/// TLS transport for the `+s` / `+ssc` schemes.
mod tls {
    use super::*;
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::SignatureScheme;
    use std::sync::Arc;

    pub fn wrap(
        tcp: TcpStream,
        host: &str,
        verify: bool,
    ) -> astria_core::Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>> {
        let config = if verify {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth()
        } else {
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert))
                .with_no_client_auth()
        };
        let name = ServerName::try_from(host.to_string())
            .or_else(|_| ServerName::try_from("localhost".to_string()))
            .map_err(|e| {
                astria_core::AstriaError::Graph(format!("invalid TLS server name {host}: {e}"))
            })?
            .to_owned();
        let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| {
            astria_core::AstriaError::Graph(format!("TLS setup for {host} failed: {e}"))
        })?;
        Ok(rustls::StreamOwned::new(conn, tcp))
    }

    /// `+ssc` verifier: encryption without identity checks, matching Neo4j's
    /// documented self-signed semantics. Only reachable for `+ssc` URLs.
    #[derive(Debug)]
    struct AcceptAnyServerCert;

    impl ServerCertVerifier for AcceptAnyServerCert {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![
                SignatureScheme::RSA_PKCS1_SHA256,
                SignatureScheme::RSA_PKCS1_SHA384,
                SignatureScheme::RSA_PKCS1_SHA512,
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::ECDSA_NISTP384_SHA384,
                SignatureScheme::ECDSA_NISTP521_SHA512,
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::RSA_PSS_SHA512,
                SignatureScheme::ED25519,
                SignatureScheme::ED448,
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A minimal Bolt server: validates the handshake, accepts HELLO and
    /// one RUN+PULL pair per pushed statement, always succeeding. Replies
    /// with `server_version` and reports each PULL's field count so tests
    /// can assert the version-gated wire shapes. Returns the RUN queries it
    /// received and the PULL field counts.
    fn spawn_mock_server(
        server_version: u32,
    ) -> (String, std::sync::mpsc::Receiver<(String, usize, usize)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();

            // Handshake: 20 bytes in, 4-byte version out.
            let mut handshake = [0u8; 20];
            sock.read_exact(&mut handshake).unwrap();
            assert_eq!(&handshake[0..4], &MAGIC);
            // The four proposals must be exactly what this client offers,
            // each spelled `00 00 <minor> <major>`.
            assert_eq!(
                &handshake[4..],
                &[
                    encode_version(5, 0).to_be_bytes(),
                    encode_version(4, 4).to_be_bytes(),
                    encode_version(4, 1).to_be_bytes(),
                    encode_version(3, 0).to_be_bytes(),
                ]
                .concat()
            );
            assert!(handshake[4..]
                .chunks_exact(4)
                .any(|v| v == server_version.to_be_bytes()));
            sock.write_all(&server_version.to_be_bytes()).unwrap();

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

                let pull = frame::read_message(&mut sock).unwrap();
                let (pull_tag, pull_fields, _) = packstream::decode_struct(&pull).unwrap();
                assert_eq!(pull_tag, tag::PULL);
                tx.send((query, fields.len(), pull_fields.len())).unwrap();

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
    fn version_encoding_is_minor_then_major() {
        // Bolt's handshake spells versions `00 00 <minor> <major>`:
        // 4.1 is 00 00 01 04, not 00 00 04 01 (which means 1.4).
        assert_eq!(encode_version(4, 4).to_be_bytes(), [0x00, 0x00, 0x04, 0x04]);
        assert_eq!(encode_version(4, 1).to_be_bytes(), [0x00, 0x00, 0x01, 0x04]);
        assert_eq!(encode_version(5, 0).to_be_bytes(), [0x00, 0x00, 0x00, 0x05]);
        assert_eq!(encode_version(3, 0).to_be_bytes(), [0x00, 0x00, 0x00, 0x03]);
    }

    #[test]
    fn url_parsing_selects_transport_by_scheme() {
        let plain = parse_url("bolt://localhost:7688").unwrap();
        assert_eq!(plain.scheme, Scheme::Plain);
        assert_eq!(plain.host, "localhost");
        assert_eq!(plain.port, 7688);
        assert_eq!(plain.credentials, None);

        let creds = parse_url("neo4j://neo4j:secret@db.example.com").unwrap();
        assert_eq!(creds.scheme, Scheme::Plain);
        assert_eq!(creds.port, 7687);
        assert_eq!(creds.credentials, Some(("neo4j".into(), "secret".into())));

        assert_eq!(
            parse_url("bolt+s://db.example.com").unwrap().scheme,
            Scheme::TlsVerified
        );
        assert_eq!(
            parse_url("neo4j+s://db.example.com").unwrap().scheme,
            Scheme::TlsVerified
        );
        assert_eq!(
            parse_url("bolt+ssc://db.example.com").unwrap().scheme,
            Scheme::TlsUnverified
        );
        assert!(parse_url("http://localhost:7687").is_err());
        assert!(parse_url("bolt://").is_err());
    }

    #[test]
    fn bolt4_uses_fielded_pull_with_n() {
        let (url, rx) = spawn_mock_server(encode_version(4, 4));
        let mut client = BoltClient::connect(&url, "neo4j", "password").unwrap();
        assert_eq!(client.version, (4, 4));

        client
            .run("UNWIND $rows AS row MERGE (n:Symbol {id: row.id})", {
                let mut params = BTreeMap::new();
                params.insert("rows".into(), Value::List(vec![Value::Integer(1)]));
                params
            })
            .unwrap();

        let (query, run_fields, pull_fields) =
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(query, "UNWIND $rows AS row MERGE (n:Symbol {id: row.id})");
        // RUN carries query + params + extra in Bolt 4+ ...
        assert_eq!(run_fields, 3);
        // ... and PULL carries one extra field: a map with `n`.
        assert_eq!(pull_fields, 1);
    }

    #[test]
    fn bolt4_1_is_accepted_from_the_wire_reply() {
        // A server that answers `00 00 01 04` (minor 1, major 4) negotiates
        // 4.1 — the encoding the old client got backwards.
        let (url, rx) = spawn_mock_server(encode_version(4, 1));
        let mut client = BoltClient::connect(&url, "neo4j", "password").unwrap();
        assert_eq!(client.version, (4, 1));
        client.run("RETURN 1", BTreeMap::new()).unwrap();
        let (_, run_fields, pull_fields) =
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(run_fields, 3);
        assert_eq!(pull_fields, 1);
    }

    #[test]
    fn bolt3_uses_fieldless_pull_all() {
        let (url, rx) = spawn_mock_server(encode_version(3, 0));
        let mut client = BoltClient::connect(&url, "neo4j", "password").unwrap();
        assert_eq!(client.version, (3, 0));

        client.run("RETURN 1", BTreeMap::new()).unwrap();

        let (query, run_fields, pull_fields) =
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(query, "RETURN 1");
        // The extra dictionary on RUN was added in v3: a 2-field RUN is the
        // v1/v2 shape and v3 servers reject it.
        assert_eq!(run_fields, 3);
        // Bolt 3 drains with the fieldless PULL_ALL.
        assert_eq!(pull_fields, 0);
    }

    #[test]
    fn unoffered_or_rejected_versions_are_errors() {
        // Server replies with a version the client never offered (2.0-style
        // `00 00 00 02`) — must be rejected, not guessed at.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut handshake = [0u8; 20];
            sock.read_exact(&mut handshake).unwrap();
            sock.write_all(&0x0000_0002u32.to_be_bytes()).unwrap();
            let _hello = frame::read_message(&mut sock);
        });
        let err = BoltClient::connect(&format!("bolt://127.0.0.1:{port}"), "n", "p").unwrap_err();
        assert!(err.to_string().contains("cannot speak"), "got: {err}");

        // And the all-zero rejection reply.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut handshake = [0u8; 20];
            sock.read_exact(&mut handshake).unwrap();
            sock.write_all(&0x0000_0000u32.to_be_bytes()).unwrap();
            let _hello = frame::read_message(&mut sock);
        });
        let err = BoltClient::connect(&format!("bolt://127.0.0.1:{port}"), "n", "p").unwrap_err();
        assert!(err.to_string().contains("rejected"), "got: {err}");
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
            sock.write_all(&encode_version(4, 4).to_be_bytes()).unwrap();
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
