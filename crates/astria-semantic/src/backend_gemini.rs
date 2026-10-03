//! The Google Gemini backend.
#![allow(unused_imports)]

use super::*;
use astria_core::AstriaError;
use astria_core::Result;
use base64::Engine as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub struct GeminiBackend {
    agent: ureq::Agent,
    api_key: String,
    model: String,
}

impl GeminiBackend {
    /// - `ASTRIA_LLM_API_KEY` (or `GEMINI_API_KEY`/`GOOGLE_API_KEY`).
    /// - `ASTRIA_LLM_MODEL` — defaults to `gemini-2.0-flash`.
    pub fn from_env() -> Result<Self> {
        let api_key = astria_core::env_var("LLM_API_KEY")
            .or_else(|| std::env::var("GEMINI_API_KEY").ok())
            .or_else(|| std::env::var("GOOGLE_API_KEY").ok())
            .ok_or_else(|| {
                AstriaError::Graph(
                    "no Gemini API key: set ASTRIA_LLM_API_KEY or GEMINI_API_KEY".into(),
                )
            })?;
        let model = astria_core::env_var("LLM_MODEL").unwrap_or_else(|| "gemini-2.0-flash".into());
        Ok(Self::new(api_key, model))
    }

    pub fn new(api_key: String, model: String) -> Self {
        Self {
            agent: build_agent(),
            api_key,
            model,
        }
    }

    /// API URL. The key is sent via the x-goog-api-key header (see
    /// `headers`), never as a URL query parameter where it would leak into
    /// logs and history.
    pub fn url(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        )
    }

    pub(crate) fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Content-Type", "application/json".into()),
            ("x-goog-api-key", self.api_key.clone()),
        ]
    }

    pub fn build_request_body(&self, content: &str, file_type: &str) -> serde_json::Value {
        serde_json::json!({
            "system_instruction": {"parts": [{"text": system_prompt(file_type)}]},
            "contents": [{"role": "user", "parts": [{"text": content}]}],
            "generationConfig": {"maxOutputTokens": enrichment::MAX_OUTPUT_TOKENS_EXTRACT}
        })
    }

    pub fn build_image_request_body(&self, image_b64: &str, media_type: &str) -> serde_json::Value {
        serde_json::json!({
            "system_instruction": {"parts": [{"text": vision_prompt()}]},
            "contents": [{"role": "user", "parts": [
                {"inline_data": {"mime_type": media_type, "data": image_b64}},
                {"text": "Extract the knowledge graph from this image."}
            ]}],
            "generationConfig": {"maxOutputTokens": enrichment::MAX_OUTPUT_TOKENS_EXTRACT}
        })
    }

    pub(crate) fn extract_raw(&self, body: serde_json::Value) -> Result<SemanticExtraction> {
        let body_str = serde_json::to_string(&body)?;
        let response = post_json(
            &self.agent,
            &self.url(),
            &self
                .headers()
                .iter()
                .map(|(k, v)| (*k, v.as_str()))
                .collect::<Vec<(&str, &str)>>(),
            &body_str,
            "Gemini",
        )?;
        let json: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| AstriaError::Graph(format!("Failed to parse Gemini response: {e}")))?;
        enrichment::record_usage(&json);
        let finish = json
            .pointer("/candidates/0/finishReason")
            .and_then(|f| f.as_str())
            .unwrap_or("");
        if finish == "MAX_TOKENS" || finish == "SAFETY" || finish == "RECITATION" {
            return Err(AstriaError::Graph(format!(
                "Gemini reply unusable (finishReason: {finish})"
            )));
        }
        let text = Self::gemini_text(&json);
        parse_extraction_text(text)
    }

    /// The first candidate's text part, shared by extraction and the
    /// auxiliary passes.
    pub(crate) fn gemini_text(json: &serde_json::Value) -> &str {
        json.get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
    }

    /// generateContent body for the auxiliary passes (smaller output cap).
    pub(crate) fn gemini_chat_body(&self, system: &str, user: &str) -> serde_json::Value {
        serde_json::json!({
            "system_instruction": {"parts": [{"text": system}]},
            "contents": [{"role": "user", "parts": [{"text": user}]}],
            "generationConfig": {"maxOutputTokens": enrichment::MAX_OUTPUT_TOKENS_COMPLETE}
        })
    }
}

impl SemanticBackend for GeminiBackend {
    fn cache_identity(&self) -> String {
        format!(
            "gemini:https://generativelanguage.googleapis.com:{}",
            self.model
        )
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
        let body_str = serde_json::to_string(&self.gemini_chat_body(system, user))?;
        let response = post_json(
            &self.agent,
            &self.url(),
            &self
                .headers()
                .iter()
                .map(|(k, v)| (*k, v.as_str()))
                .collect::<Vec<(&str, &str)>>(),
            &body_str,
            "Gemini",
        )?;
        let json: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| AstriaError::Graph(format!("Failed to parse Gemini response: {e}")))?;
        enrichment::record_usage(&json);
        Ok(Self::gemini_text(&json).to_string())
    }
}

// ---------------------------------------------------------------------------
// JevJudgeBackend (TypeSafe System One judge layer over a completion engine)
// ---------------------------------------------------------------------------
