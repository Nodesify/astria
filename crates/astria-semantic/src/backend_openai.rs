//! The OpenAI-compatible backend: OpenAI, DeepSeek, Ollama (/v1),
//! LM Studio, and custom providers via a configurable base URL.
#![allow(unused_imports)]

use super::*;
use astria_core::AstriaError;
use astria_core::Result;
use base64::Engine as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Works with OpenAI, DeepSeek, Ollama (/v1), LM Studio, and custom
/// OpenAI-compatible providers via a configurable base URL.
pub struct OpenAiBackend {
    agent: ureq::Agent,
    api_key: Option<String>,
    base_url: String,
    model: String,
}

impl OpenAiBackend {
    /// - `ASTRIA_LLM_BASE_URL` (or `OPENAI_BASE_URL`) — defaults to OpenAI.
    /// - `ASTRIA_LLM_API_KEY` (or `OPENAI_API_KEY`) — optional for local
    ///   servers like Ollama.
    /// - `ASTRIA_LLM_MODEL` — defaults to `gpt-4o-mini`.
    pub fn from_env() -> Result<Self> {
        let base_url = astria_core::env_var("LLM_BASE_URL")
            .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
            .unwrap_or_else(|| "https://api.openai.com/v1".into());
        let api_key =
            astria_core::env_var("LLM_API_KEY").or_else(|| std::env::var("OPENAI_API_KEY").ok());
        let model = astria_core::env_var("LLM_MODEL").unwrap_or_else(|| "gpt-4o-mini".into());
        if api_key.is_none() && base_url.contains("api.openai.com") {
            return Err(AstriaError::Graph(
                "no API key: set ASTRIA_LLM_API_KEY or OPENAI_API_KEY".into(),
            ));
        }
        if api_key.is_some() && base_url.starts_with("http://") && !is_local_base_url(&base_url) {
            eprintln!(
                "warning: ASTRIA_LLM_BASE_URL uses plain http ({base_url}); \
                 the API key is sent unencrypted"
            );
        }
        Ok(Self {
            agent: build_agent(),
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
        })
    }

    pub fn new(api_key: Option<String>, base_url: String, model: String) -> Self {
        Self {
            agent: build_agent(),
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
        }
    }

    pub fn build_request_body(&self, content: &str, file_type: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": [
                {"role": "system", "content": system_prompt(file_type)},
                {"role": "user", "content": content}
            ]
        })
    }

    pub fn build_image_request_body(&self, image_b64: &str, media_type: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": [
                {"role": "system", "content": vision_prompt()},
                {"role": "user", "content": [
                    {"type": "text", "text": "Extract the knowledge graph from this image."},
                    {"type": "image_url", "image_url": {"url": format!("data:{media_type};base64,{image_b64}")}}
                ]}
            ]
        })
    }

    pub(crate) fn headers(&self) -> Vec<(&'static str, String)> {
        let mut headers = vec![("Content-Type", "application/json".to_string())];
        if let Some(key) = &self.api_key {
            headers.push(("Authorization", format!("Bearer {key}")));
        }
        headers
    }

    pub(crate) fn extract_raw(&self, body: serde_json::Value) -> Result<SemanticExtraction> {
        let body_str = serde_json::to_string(&body)?;
        let url = format!("{}/chat/completions", self.base_url);
        let headers = self.headers();
        let hdr: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let response = post_json(&self.agent, &url, &hdr, &body_str, "OpenAI-compatible")?;
        let json: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| AstriaError::Graph(format!("Failed to parse OpenAI response: {e}")))?;
        enrichment::record_usage(&json);
        let text = json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        Ok(parse_extraction_text(text))
    }

    /// Chat-completions body for the auxiliary passes (smaller output cap).
    pub(crate) fn chat_body(&self, system: &str, user: &str, max_tokens: u32) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ]
        })
    }

    pub(crate) fn complete_text(&self, body: serde_json::Value) -> Result<String> {
        let body_str = serde_json::to_string(&body)?;
        let url = format!("{}/chat/completions", self.base_url);
        let headers = self.headers();
        let hdr: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let response = post_json(&self.agent, &url, &hdr, &body_str, "OpenAI-compatible")?;
        let json: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| AstriaError::Graph(format!("Failed to parse OpenAI response: {e}")))?;
        enrichment::record_usage(&json);
        Ok(json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string())
    }
}

impl SemanticBackend for OpenAiBackend {
    fn cache_identity(&self) -> String {
        format!("openai:{}:{}", self.base_url, self.model)
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
        self.complete_text(self.chat_body(system, user, 1024))
    }
}

// ---------------------------------------------------------------------------
// GeminiBackend (Google Generative Language API)
// ---------------------------------------------------------------------------
