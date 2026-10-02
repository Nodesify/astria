//! Live Neo4j integration tests — `#[ignore]`d by default because they need
//! a running server. The unit tests cover PackStream encoding and frame
//! parsing in isolation; only a real server exercises the handshake (HELLO),
//! the RUN/PULL round trip, and the auth-failure path end to end.
//!
//! Run locally against Docker:
//!
//! ```text
//! docker run --rm -p 7687:7687 -e NEO4J_AUTH=neo4j/testpassword neo4j:5
//! ASTRIA_BOLT_TEST_URL="neo4j://localhost:7687" \
//!   ASTRIA_BOLT_TEST_USER=neo4j ASTRIA_BOLT_TEST_PASS=testpassword \
//!   cargo test -p astria-bolt --test live_neo4j -- --ignored
//! ```
//!
//! Unset environment variables skip the test with a pass, so running the
//! whole suite with `-- --ignored` on a machine without a server stays green.

use std::collections::BTreeMap;

fn non_empty_env(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

#[test]
#[ignore = "needs a live Neo4j server (see the module docs for the Docker one-liner)"]
fn handshake_and_round_trip_against_live_server() {
    let Some(url) = non_empty_env("ASTRIA_BOLT_TEST_URL") else {
        eprintln!("skipped: ASTRIA_BOLT_TEST_URL is not set");
        return;
    };
    let user = std::env::var("ASTRIA_BOLT_TEST_USER").unwrap_or_else(|_| "neo4j".to_string());
    let pass =
        std::env::var("ASTRIA_BOLT_TEST_PASS").unwrap_or_else(|_| "testpassword".to_string());

    let mut client = astria_bolt::BoltClient::connect(&url, &user, &pass)
        .expect("HELLO handshake against a live server must succeed");

    let mut params = BTreeMap::new();
    params.insert(
        "name".to_string(),
        astria_bolt::packstream::Value::String("astria".to_string()),
    );
    // Parametrized query exercises PackStream string + map packing and the
    // RUN/PULL response parsing on the server's real bytes.
    client
        .run("RETURN $name AS greeting", params)
        .expect("RUN/PULL round trip must succeed");
}

#[test]
#[ignore = "needs a live Neo4j server (see the module docs for the Docker one-liner)"]
fn wrong_password_is_an_error_not_a_panic() {
    let Some(url) = non_empty_env("ASTRIA_BOLT_TEST_URL") else {
        eprintln!("skipped: ASTRIA_BOLT_TEST_URL is not set");
        return;
    };
    let user = std::env::var("ASTRIA_BOLT_TEST_USER").unwrap_or_else(|_| "neo4j".to_string());

    let result = astria_bolt::BoltClient::connect(&url, &user, "definitely-not-the-password");
    assert!(
        result.is_err(),
        "bad credentials must surface as an error, not a successful client"
    );
}
