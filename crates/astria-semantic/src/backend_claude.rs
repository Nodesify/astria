//! The Anthropic Claude backend.
#![allow(unused_imports)]

use super::*;
use astria_core::AstriaError;
use astria_core::Result;
use base64::Engine as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub struct ClaudeBackend {
    agent: ureq::Agent,
    api_key: String,
    model: String,
}

impl ClaudeBackend {
    /// - `ASTRIA_LLM_API_KEY` (or legacy `GRAPHIFY_LLM_API_KEY`) — required, the Anthropic API key.
    /// - `ASTRIA_LLM_MODEL` — optional, defaults to `claude-sonnet-4-20250514`.
    pub fn from_env() -> Result<Self> {
        let api_key = astria_core::env_var("LLM_API_KEY").ok_or_else(|| {
            AstriaError::Graph("ASTRIA_LLM_API_KEY environment variable is not set".into())
        })?;
        let model =
            astria_core::env_var("LLM_MODEL").unwrap_or_else(|| "claude-sonnet-4-20250514".into());
        Ok(Self::new(api_key, model))
    }

    pub fn new(api_key: String, model: String) -> Self {
        Self {
            agent: build_agent(),
            api_key,
            model,
        }
    }

    pub fn build_request_body(&self, content: &str, file_type: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": enrichment::MAX_OUTPUT_TOKENS_EXTRACT,
            "system": system_prompt(file_type),
            "messages": [
                {"role": "user", "content": content}
            ]
        })
    }

    pub fn build_image_request_body(&self, image_b64: &str, media_type: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": enrichment::MAX_OUTPUT_TOKENS_EXTRACT,
            "system": vision_prompt(),
            "messages": [
                {"role": "user", "content": [
                    {"type": "image", "source": {"type": "base64", "media_type": media_type, "data": image_b64}},
                    {"type": "text", "text": "Extract the knowledge graph from this image."}
                ]}
            ]
        })
    }

    pub(crate) fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Content-Type", "application/json".into()),
            ("x-api-key", self.api_key.clone()),
            ("anthropic-version", "2023-06-01".into()),
        ]
    }

    /// Single-shot API call for one piece of content.
    pub(crate) fn extract_raw(&self, content: &str, file_type: &str) -> Result<SemanticExtraction> {
        let body = serde_json::to_string(&self.build_request_body(content, file_type))?;
        let headers = self.headers();
        let hdr: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let response = post_json(
            &self.agent,
            "https://api.anthropic.com/v1/messages",
            &hdr,
            &body,
            "Claude",
        )?;
        self.parse_messages_response(&response)
    }

    pub(crate) fn parse_messages_response(&self, response: &str) -> Result<SemanticExtraction> {
        let json: serde_json::Value = serde_json::from_str(response)
            .map_err(|e| AstriaError::Graph(format!("Failed to parse Claude API response: {e}")))?;
        enrichment::record_usage(&json);
        // Refusals and truncations report through stop_reason; neither is
        // a usable extraction.
        let stop = json
            .get("stop_reason")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if stop == "max_tokens" || stop == "refusal" {
            return Err(AstriaError::Graph(format!(
                "Claude reply unusable (stop_reason: {stop})"
            )));
        }
        let text = json
            .get("content")
            .and_then(|c| c.get(0))
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| AstriaError::Graph("Claude reply had no text block".into()))?;
        parse_extraction_text(text)
    }

    /// Single-turn Messages-API body for the auxiliary passes.
    pub(crate) fn claude_chat_body(&self, system: &str, user: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": enrichment::MAX_OUTPUT_TOKENS_COMPLETE,
            "system": system,
            "messages": [
                {"role": "user", "content": user}
            ]
        })
    }

    pub(crate) fn claude_complete_text(&self, body: serde_json::Value) -> Result<String> {
        let body_str = serde_json::to_string(&body)?;
        let headers = self.headers();
        let hdr: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let response = post_json(
            &self.agent,
            "https://api.anthropic.com/v1/messages",
            &hdr,
            &body_str,
            "Claude",
        )?;
        let json: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| AstriaError::Graph(format!("Failed to parse Claude API response: {e}")))?;
        enrichment::record_usage(&json);
        Ok(json
            .get("content")
            .and_then(|c| c.get(0))
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string())
    }

    pub(crate) fn extract_image_raw(
        &self,
        image_bytes: &[u8],
        media_type: &str,
    ) -> Result<SemanticExtraction> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(image_bytes);
        let body = serde_json::to_string(&self.build_image_request_body(&encoded, media_type))?;
        let headers = self.headers();
        let hdr: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let response = post_json(
            &self.agent,
            "https://api.anthropic.com/v1/messages",
            &hdr,
            &body,
            "Claude",
        )?;
        self.parse_messages_response(&response)
    }
}

impl SemanticBackend for ClaudeBackend {
    fn cache_identity(&self) -> String {
        format!("claude:https://api.anthropic.com:{}", self.model)
    }
    fn extract_semantic(&self, content: &str, file_type: &str) -> Result<SemanticExtraction> {
        extract_content_chunked(content, file_type, |c, ft| self.extract_raw(c, ft))
    }

    fn extract_semantic_from_image(
        &self,
        image_bytes: &[u8],
        media_type: &str,
    ) -> Result<SemanticExtraction> {
        self.extract_image_raw(image_bytes, media_type)
    }

    fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.claude_complete_text(self.claude_chat_body(system, user))
    }
}

// ---------------------------------------------------------------------------
// OpenAiBackend (any OpenAI-compatible endpoint)
// ---------------------------------------------------------------------------
