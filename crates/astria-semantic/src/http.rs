//! Shared HTTP plumbing: POST with exponential backoff and Retry-After
//! handling, the shared ureq agent, and local-endpoint detection.
#![allow(unused_imports)]

use super::*;
use astria_core::AstriaError;
use astria_core::Result;
use base64::Engine as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// POST with exponential backoff on 429/5xx (honoring Retry-After when the
/// server sends one), shared by all backends.
///
/// Status handling lives entirely in the success branch: the shared agent
/// is configured with `http_status_as_error(false)`, so every response —
/// 2xx through 5xx — arrives as `Ok`, and this one place decides what is
/// retryable. Transport failures (connection refused, TLS, timeouts) arrive
/// as `Err` and are always retryable. All delays are milliseconds; never
/// mixed with seconds.
pub(crate) fn post_json(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
    body: &str,
    backend_name: &str,
) -> Result<String> {
    let max_retries = 3;
    let backoff_ms = |attempt: u32| 500u64 * 2u64.pow(attempt);
    for attempt in 0..=max_retries {
        let mut request = agent.post(url);
        for &(k, v) in headers {
            request = request.header(k, v);
        }
        match request.send(body) {
            Ok(resp) => {
                let status = resp.status();
                let retry_after_ms = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .map(|s| (s * 1000).min(MAX_RETRY_AFTER_SECS * 1000));
                let response_body = resp.into_body().read_to_string().unwrap_or_default();
                if status.is_success() {
                    return Ok(response_body);
                }
                let diagnostic = diagnostic_tail(&response_body);
                // Non-retryable client errors (auth, validation): fail now —
                // retrying burns budget and hides the real problem.
                if status.is_client_error() && status.as_u16() != 429 {
                    return Err(AstriaError::Graph(format!(
                        "{backend_name} API returned {status}: {diagnostic}"
                    )));
                }
                // 429 and 5xx are retryable (RateLimited has no is_ method).
                if attempt == max_retries {
                    return Err(AstriaError::Graph(format!(
                        "{backend_name} API returned {status}: {diagnostic}"
                    )));
                }
                let delay = retry_after_ms.unwrap_or(backoff_ms(attempt as u32));
                std::thread::sleep(Duration::from_millis(delay));
            }
            Err(e) => {
                if attempt == max_retries {
                    return Err(AstriaError::Graph(format!(
                        "{backend_name} API request failed: {e}"
                    )));
                }
                std::thread::sleep(Duration::from_millis(backoff_ms(attempt as u32)));
            }
        }
    }
    unreachable!("loop returns on its final attempt")
}

/// Last ~400 characters of an error body — enough to diagnose, never a
/// multi-megabyte dump in a log line.
fn diagnostic_tail(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= 400 {
        trimmed.to_string()
    } else {
        let tail: String = trimmed
            .chars()
            .rev()
            .take(400)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("…{tail}")
    }
}

pub(crate) fn build_agent() -> ureq::Agent {
    ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(60)))
        // Statuses must reach post_json's single decision point; ureq's
        // default would turn every 4xx/5xx into a transport error here and
        // make auth/validation failures look retryable.
        .http_status_as_error(false)
        .build()
        .new_agent()
}

/// True when a base URL targets the local machine, where plain http is the
/// normal, safe configuration (Ollama, LM Studio, vLLM) and there is no
/// network to eavesdrop on.
pub(crate) fn is_local_base_url(base_url: &str) -> bool {
    let after_scheme = base_url
        .strip_prefix("http://")
        .or_else(|| base_url.strip_prefix("https://"))
        .unwrap_or(base_url);
    let authority = after_scheme.split(['/', '?']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    let host = if let Some(rest) = authority.strip_prefix('[') {
        // Bracketed IPv6 literal; strip the port after the closing bracket.
        rest.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    let host = host.to_lowercase();
    matches!(host.as_str(), "localhost" | "0.0.0.0" | "::1") || host.starts_with("127.")
}

// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// ClaudeBackend (Anthropic Messages API)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Scripted HTTP server: answers each connection with the next canned
    /// status in order; records how many requests it served. Retry-After: 0
    /// keeps retries fast.
    fn spawn_scripted(
        statuses: &[u16],
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let statuses = statuses.to_vec();
        let served_clone = served.clone();
        std::thread::spawn(move || {
            for status in statuses {
                let (mut sock, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(_) => return,
                };
                // Drain the full request (head + body) before responding:
                // closing with unread data sends a TCP RST that can destroy
                // the client's pending response read — a flake under load.
                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
                let header_end = loop {
                    let n = match sock.read(&mut buf) {
                        Ok(0) | Err(_) => break None,
                        Ok(n) => n,
                    };
                    request.extend_from_slice(&buf[..n]);
                    if let Some(pos) = find_header_end(&request) {
                        break Some(pos);
                    }
                };
                if let Some(pos) = header_end {
                    let head = String::from_utf8_lossy(&request[..pos]).to_lowercase();
                    let content_length: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    while request.len() < pos + content_length {
                        let n = match sock.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => n,
                        };
                        request.extend_from_slice(&buf[..n]);
                    }
                }
                served_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let retry_after = if status == 429 || status >= 500 {
                    "Retry-After: 0\r\n"
                } else {
                    ""
                };
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{retry_after}Content-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                );
                let _ = sock.write_all(head.as_bytes());
                let _ = sock.flush();
            }
        });
        (format!("http://127.0.0.1:{port}/v1/x"), served)
    }

    fn find_header_end(request: &[u8]) -> Option<usize> {
        request
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|p| p + 4)
    }

    #[test]
    fn server_errors_retry_until_success() {
        let (url, served) = spawn_scripted(&[500, 500, 200]);
        let agent = build_agent();
        let body = post_json(&agent, &url, &[], "{}", "test").unwrap();
        assert_eq!(body, "{}");
        assert_eq!(served.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[test]
    fn rate_limit_retry_then_success() {
        let (url, served) = spawn_scripted(&[429, 200]);
        let agent = build_agent();
        assert!(post_json(&agent, &url, &[], "{}", "test").is_ok());
        assert_eq!(served.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn auth_errors_fail_immediately_without_retry() {
        // A 401 must not be retried: retrying auth failures was the old
        // ureq-status-as-error behavior.
        let (url, served) = spawn_scripted(&[401, 200]);
        let agent = build_agent();
        let err = post_json(&agent, &url, &[], "{}", "test").unwrap_err();
        assert!(err.to_string().contains("401"), "got: {err}");
        assert_eq!(
            served.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "client errors must fail on the first attempt"
        );
    }

    #[test]
    fn exhausted_retries_surface_the_status() {
        let (url, served) = spawn_scripted(&[500, 500, 500, 500]);
        let agent = build_agent();
        let err = post_json(&agent, &url, &[], "{}", "test").unwrap_err();
        assert!(err.to_string().contains("500"), "got: {err}");
        assert_eq!(served.load(std::sync::atomic::Ordering::SeqCst), 4);
    }
}
