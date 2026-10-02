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
pub(crate) fn post_json(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
    body: &str,
    backend_name: &str,
) -> Result<String> {
    let max_retries = 3;
    let mut last_err = None;
    for attempt in 0..=max_retries {
        let mut request = agent.post(url);
        for &(k, v) in headers {
            request = request.header(k, v);
        }
        match request.send(body) {
            Ok(resp) => {
                let status = resp.status();
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .map(|s| s.min(MAX_RETRY_AFTER_SECS));
                let response_body = resp.into_body().read_to_string().unwrap_or_default();
                if status.is_client_error() && status.as_u16() != 429 {
                    return Err(AstriaError::Graph(format!(
                        "{backend_name} API returned {status}: {response_body}"
                    )));
                }
                if status.is_server_error() || status.as_u16() == 429 {
                    last_err = Some(format!("{backend_name} API returned {status}"));
                    if attempt < max_retries {
                        let delay = retry_after.unwrap_or_else(|| 500 * 2u64.pow(attempt as u32));
                        std::thread::sleep(Duration::from_millis(delay * 1000));
                        continue;
                    }
                    return Err(AstriaError::Graph(last_err.unwrap()));
                }
                return Ok(response_body);
            }
            Err(e) => {
                last_err = Some(format!("{backend_name} API request failed: {e}"));
                if attempt < max_retries {
                    std::thread::sleep(Duration::from_millis(500 * 2u64.pow(attempt as u32)));
                    continue;
                }
            }
        }
    }
    Err(AstriaError::Graph(last_err.unwrap()))
}

pub(crate) fn build_agent() -> ureq::Agent {
    ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(60)))
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
