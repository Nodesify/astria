//! AWS Bedrock backend (Converse API) — first-class IAM auth via SigV4.
//!
//! The OpenAI-compatible path cannot reach Bedrock: Bedrock signs requests
//! with AWS SigV4 (HMAC over a canonical request), which no bearer-token
//! client can produce. This backend implements the signing (sigv4.rs) and
//! speaks Bedrock's Converse wire format directly, so AWS credentials
//! (static keys or `AWS_SESSION_TOKEN` temporary credentials) work without
//! an external proxy.
#![allow(unused_imports)]

use super::*;
use astria_core::AstriaError;
use astria_core::Result;
use base64::Engine as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub struct BedrockBackend {
    agent: ureq::Agent,
    region: String,
    model: String,
    access_key: String,
    secret_key: String,
    session_token: Option<String>,
    /// Precomputed, percent-encoded `/model/{id}/converse` path — the SigV4
    /// canonical request signs this exact string.
    encoded_path: String,
}

impl BedrockBackend {
    /// - `ASTRIA_AWS_REGION` / `AWS_REGION` / `AWS_DEFAULT_REGION` — region.
    /// - `ASTRIA_AWS_ACCESS_KEY_ID` / `AWS_ACCESS_KEY_ID` — access key.
    /// - `ASTRIA_AWS_SECRET_ACCESS_KEY` / `AWS_SECRET_ACCESS_KEY` — secret.
    /// - `ASTRIA_AWS_SESSION_TOKEN` / `AWS_SESSION_TOKEN` — optional (temporary credentials).
    /// - `ASTRIA_LLM_MODEL` — Bedrock model id, e.g.
    ///   `anthropic.claude-3-5-sonnet-20241022-v2:0` or `us.amazon.nova-pro-v1:0`.
    pub fn from_env() -> Result<Self> {
        let region = astria_core::env_var("AWS_REGION")
            .or_else(|| std::env::var("AWS_REGION").ok())
            .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
            .filter(|r| !r.trim().is_empty())
            .ok_or_else(|| {
                AstriaError::Graph(
                    "no AWS region: set ASTRIA_AWS_REGION (or AWS_REGION / AWS_DEFAULT_REGION)"
                        .into(),
                )
            })?;
        let access_key = astria_core::env_var("AWS_ACCESS_KEY_ID")
            .or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok())
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| {
                AstriaError::Graph(
                    "no AWS credentials: set ASTRIA_AWS_ACCESS_KEY_ID + ASTRIA_AWS_SECRET_ACCESS_KEY \
                     (or the standard AWS_* variables)"
                        .into(),
                )
            })?;
        let secret_key = astria_core::env_var("AWS_SECRET_ACCESS_KEY")
            .or_else(|| std::env::var("AWS_SECRET_ACCESS_KEY").ok())
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| {
                AstriaError::Graph("no AWS secret access key: set ASTRIA_AWS_SECRET_ACCESS_KEY".into())
            })?;
        let session_token = astria_core::env_var("AWS_SESSION_TOKEN")
            .or_else(|| std::env::var("AWS_SESSION_TOKEN").ok())
            .filter(|t| !t.trim().is_empty());
        let model = astria_core::env_var("LLM_MODEL")
            .filter(|m| !m.trim().is_empty())
            .ok_or_else(|| {
                AstriaError::Graph(
                    "no Bedrock model: set ASTRIA_LLM_MODEL to a Bedrock model id \
                     (e.g. anthropic.claude-3-5-sonnet-20241022-v2:0 or us.amazon.nova-pro-v1:0)"
                        .into(),
                )
            })?;
        Ok(Self {
            agent: build_agent(),
            region: region.trim().to_string(),
            model: model.trim().to_string(),
            access_key: access_key.trim().to_string(),
            secret_key: secret_key.trim().to_string(),
            session_token,
            encoded_path: format!(
                "/model/{}/converse",
                sigv4::encode_model_id(model.trim())
            ),
        })
    }

    pub fn new(
        region: String,
        model: String,
        access_key: String,
        secret_key: String,
        session_token: Option<String>,
    ) -> Self {
        let encoded_path = format!("/model/{}/converse", sigv4::encode_model_id(&model));
        Self {
            agent: build_agent(),
            region: region.clone(),
            model,
            access_key,
            secret_key,
            session_token,
            encoded_path,
        }
    }

    pub fn url(&self) -> String {
        format!(
            "https://bedrock-runtime.{}.amazonaws.com{}",
            self.region, self.encoded_path
        )
    }

    /// SigV4-signed headers for one POST. `x-amz-content-sha256` and the
    /// signature cover the exact payload bytes sent.
    fn sign_headers(&self, payload: &[u8]) -> Result<Vec<(String, String)>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let (amz_date, short_date) = sigv4::amz_dates(now);
        let host = format!("bedrock-runtime.{}.amazonaws.com", self.region);
        let request = sigv4::SigV4Request {
            method: "POST",
            host: &host,
            path: &self.encoded_path,
            payload,
            access_key: &self.access_key,
            secret_key: &self.secret_key,
            session_token: self.session_token.as_deref(),
            region: &self.region,
            service: "bedrock",
            amz_date: &amz_date,
            short_date: &short_date,
        };
        let mut headers = request.sign();
        headers.push(("Content-Type".to_string(), "application/json".to_string()));
        Ok(headers)
    }

    pub fn build_request_body(&self, content: &str, file_type: &str) -> serde_json::Value {
        serde_json::json!({
            "system": [{"text": system_prompt(file_type)}],
            "messages": [{"role": "user", "content": [{"text": content}]}],
            "inferenceConfig": {"maxTokens": 4096}
        })
    }

    pub fn build_image_request_body(&self, image_b64: &str, media_type: &str) -> serde_json::Value {
        let format = media_type.strip_prefix("image/").unwrap_or("png");
        serde_json::json!({
            "system": [{"text": vision_prompt()}],
            "messages": [{"role": "user", "content": [
                {"image": {"format": format, "source": {"bytes": image_b64}}},
                {"text": "Extract the knowledge graph from this image."}
            ]}],
            "inferenceConfig": {"maxTokens": 4096}
        })
    }

    pub(crate) fn extract_raw(&self, body: serde_json::Value) -> Result<SemanticExtraction> {
        self.converse_text(&body).map(|text| parse_extraction_text(&text))
    }

    /// One Converse call: sign, send with the shared retry/backoff POST, and
    /// return the assistant text (all text content parts joined).
    fn converse_text(&self, body: &serde_json::Value) -> Result<String> {
        let body_str = serde_json::to_string(body)?;
        let headers = self.sign_headers(body_str.as_bytes())?;
        let hdr: Vec<(&str, &str)> =
            headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let response = post_json(&self.agent, &self.url(), &hdr, &body_str, "AWS Bedrock")?;
        let json: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| AstriaError::Graph(format!("Failed to parse Bedrock response: {e}")))?;
        enrichment::record_usage(&json);
        let text = json
            .pointer("/output/message/content")
            .and_then(|c| c.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.is_empty() && json.get("output").is_none() {
            return Err(AstriaError::Graph(format!(
                "Bedrock response missing output (model {}): {}",
                self.model,
                &response.chars().take(300).collect::<String>()
            )));
        }
        Ok(text)
    }

    /// Single-shot completion for the auxiliary passes (community naming,
    /// deep concept linking).
    fn chat_body(&self, system: &str, user: &str, max_tokens: u32) -> serde_json::Value {
        serde_json::json!({
            "system": [{"text": system}],
            "messages": [{"role": "user", "content": [{"text": user}]}],
            "inferenceConfig": {"maxTokens": max_tokens}
        })
    }
}

impl SemanticBackend for BedrockBackend {
    fn cache_identity(&self) -> String {
        format!("bedrock:{}:{}", self.region, self.model)
    }

    fn extract_semantic(&self, content: &str, file_type: &str) -> Result<SemanticExtraction> {
        extract_content_chunked(content, file_type, |c, ft| {
            self.extract_raw(self.build_request_body(c, ft))
        })
    }

    fn extract_semantic_from_image(
        &self,
        image_bytes: &[u8],
        media_type: &str,
    ) -> Result<SemanticExtraction> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(image_bytes);
        self.extract_raw(self.build_image_request_body(&encoded, media_type))
    }

    fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.converse_text(&self.chat_body(system, user, 1024))
    }
}
