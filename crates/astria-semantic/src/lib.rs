// astria-semantic: LLM-based semantic extraction for knowledge graph
// enrichment. Multi-backend (parity with upstream astria v8 llm.py):
// Anthropic Claude, any OpenAI-compatible endpoint (OpenAI, DeepSeek,
// Ollama, custom providers), and Google Gemini — plus vision/image
// extraction through each backend's multimodal API.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use astria_core::AstriaError;
use astria_core::Result;
use base64::Engine as _;

/// Maximum image size sent to vision endpoints (5 MB, matching upstream).
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

/// Long content is extracted in chunks of this many characters (about 6k
/// tokens) instead of one oversized request that would blow the context
/// window or silently truncate.
const MAX_CHUNK_CHARS: usize = 24_000;
/// API-call budget for one file: at most this many chunks are extracted.
const MAX_CHUNKS: usize = 8;
/// Ceiling for a server-supplied Retry-After (seconds) before falling back
/// to the default backoff schedule.
const MAX_RETRY_AFTER_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A semantically extracted node (topic, concept, entity).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SemanticNode {
    pub id: String,
    pub label: String,
    pub summary: String,
    pub node_type: String,
}

/// A semantically extracted edge (relationship between two nodes).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SemanticEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
}

/// The result of semantic extraction on a single piece of content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SemanticExtraction {
    pub nodes: Vec<SemanticNode>,
    pub edges: Vec<SemanticEdge>,
}

impl SemanticExtraction {
    pub fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Backend trait
// ---------------------------------------------------------------------------

/// Trait for semantic extraction backends.
pub trait SemanticBackend {
    fn extract_semantic(&self, content: &str, file_type: &str) -> Result<SemanticExtraction>;

    /// Vision extraction: describe concepts from image bytes (PNG/JPEG/
    /// WEBP/GIF). Backends without multimodal support return an error.
    fn extract_semantic_from_image(
        &self,
        _image_bytes: &[u8],
        _media_type: &str,
    ) -> Result<SemanticExtraction> {
        Err(AstriaError::Graph(
            "this backend does not support image extraction".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Shared prompt + parsing
// ---------------------------------------------------------------------------

fn system_prompt(file_type: &str) -> String {
    format!(
        "You are a knowledge graph extraction engine. Given the following {file_type} content, \
         extract semantic topics, concepts, and entities as nodes, and the relationships \
         between them as edges. Respond ONLY with valid JSON in this exact format:\n\
         {{\"nodes\": [{{\"id\": \"...\", \"label\": \"...\", \"summary\": \"...\", \"node_type\": \"...\"}}], \
         \"edges\": [{{\"source\": \"node_id\", \"target\": \"node_id\", \"relation\": \"...\"}}]}}\n\
         Use concise lowercase IDs (e.g. \"error_handling\"). \
         node_type should be one of: concept, entity, pattern, module, function.\n\
         relation should be one of: depends_on, implements, relates_to, contains, uses.\n\
         Return an empty JSON object if the content is too short or uninformative."
    )
}

fn vision_prompt() -> String {
    "You are a knowledge graph extraction engine. The user message contains an image \
     (a screenshot, diagram, whiteboard photo, chart, or slide). Extract the visible \
     concepts, entities, and their relationships as a knowledge graph. Respond ONLY \
     with valid JSON in the same format used for text extraction: \
     {\"nodes\": [...], \"edges\": [...]}. If the image has no meaningful content, \
     return an empty JSON object."
        .to_string()
}

/// Parse the model's text reply into an extraction, tolerating surrounding
/// prose by falling back to the outermost {...} span. The result is always
/// sanitized (see `sanitize_extraction`).
fn parse_extraction_text(text: &str) -> SemanticExtraction {
    if text.trim().is_empty() {
        return SemanticExtraction::empty();
    }
    if let Ok(parsed) = serde_json::from_str::<SemanticExtraction>(text.trim()) {
        return sanitize_extraction(parsed);
    }
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        if let Ok(parsed) = serde_json::from_str::<SemanticExtraction>(&text[start..=end]) {
            return sanitize_extraction(parsed);
        }
    }
    SemanticExtraction::empty()
}

/// node_type values the schema allows; anything else is clamped.
const ALLOWED_NODE_TYPES: &[&str] = &["concept", "entity", "pattern", "module", "function"];
/// relation values the schema allows; anything else is clamped.
const ALLOWED_RELATIONS: &[&str] = &["depends_on", "implements", "relates_to", "contains", "uses"];

/// Enforce output discipline on model responses: drop empty/duplicate
/// nodes, clamp node_type/relation to the schema enums, and drop edges
/// whose endpoints were not returned as nodes (they would become dangling
/// stubs in the graph).
fn sanitize_extraction(mut ext: SemanticExtraction) -> SemanticExtraction {
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    ext.nodes.retain(|node| {
        !node.id.trim().is_empty()
            && !node.label.trim().is_empty()
            && seen_ids.insert(node.id.trim().to_string())
    });
    for node in &mut ext.nodes {
        node.id = node.id.trim().to_string();
        if !ALLOWED_NODE_TYPES.contains(&node.node_type.as_str()) {
            node.node_type = "concept".to_string();
        }
    }
    let valid_ids: std::collections::HashSet<&str> =
        ext.nodes.iter().map(|n| n.id.as_str()).collect();

    let mut seen_edges: std::collections::HashSet<(String, String, String)> =
        std::collections::HashSet::new();
    ext.edges.retain(|edge| {
        edge.source != edge.target
            && valid_ids.contains(edge.source.as_str())
            && valid_ids.contains(edge.target.as_str())
            && seen_edges.insert((
                edge.source.clone(),
                edge.target.clone(),
                edge.relation.clone(),
            ))
    });
    for edge in &mut ext.edges {
        if !ALLOWED_RELATIONS.contains(&edge.relation.as_str()) {
            edge.relation = "relates_to".to_string();
        }
    }
    ext
}

/// Split `content` into chunks of at most MAX_CHUNK_CHARS characters,
/// preferring line boundaries so extractions see coherent code blocks.
fn split_chunks(content: &str) -> Vec<String> {
    if content.chars().count() <= MAX_CHUNK_CHARS {
        return vec![content.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for line in content.split_inclusive('\n') {
        let line_len = line.chars().count();
        if current_len > 0 && current_len + line_len > MAX_CHUNK_CHARS {
            chunks.push(std::mem::take(&mut current));
            current_len = 0;
        }
        current_len += line_len;
        current.push_str(line);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Merge per-chunk extractions: nodes deduplicated by id (first wins),
/// edges concatenated. Sanitization happens afterwards against the merged
/// node set so cross-chunk references survive.
fn merge_extractions(parts: Vec<SemanticExtraction>) -> SemanticExtraction {
    let mut merged = SemanticExtraction::empty();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for part in parts {
        for node in part.nodes {
            if seen.insert(node.id.clone()) {
                merged.nodes.push(node);
            }
        }
        merged.edges.extend(part.edges);
    }
    merged
}

/// Extract from possibly-long content: chunk it, extract each chunk, merge,
/// then sanitize the combined result. Bounded to MAX_CHUNKS API calls.
fn extract_content_chunked<F>(content: &str, file_type: &str, raw: F) -> Result<SemanticExtraction>
where
    F: Fn(&str, &str) -> Result<SemanticExtraction>,
{
    let chunks = split_chunks(content);
    let mut parts = Vec::new();
    for chunk in chunks.iter().take(MAX_CHUNKS) {
        parts.push(raw(chunk, file_type)?);
    }
    Ok(sanitize_extraction(merge_extractions(parts)))
}

/// POST with exponential backoff on 429/5xx (honoring Retry-After when the
/// server sends one), shared by all backends.
fn post_json(
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

fn build_agent() -> ureq::Agent {
    ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .new_agent()
}

/// True when a base URL targets the local machine, where plain http is the
/// normal, safe configuration (Ollama, LM Studio, vLLM) and there is no
/// network to eavesdrop on.
fn is_local_base_url(base_url: &str) -> bool {
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
// NoopBackend
// ---------------------------------------------------------------------------

/// A no-op backend that always returns empty extractions.
pub struct NoopBackend;

impl SemanticBackend for NoopBackend {
    fn extract_semantic(&self, _content: &str, _file_type: &str) -> Result<SemanticExtraction> {
        Ok(SemanticExtraction::empty())
    }
}

// ---------------------------------------------------------------------------
// ClaudeBackend (Anthropic Messages API)
// ---------------------------------------------------------------------------

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
            "max_tokens": 4096,
            "system": system_prompt(file_type),
            "messages": [
                {"role": "user", "content": content}
            ]
        })
    }

    pub fn build_image_request_body(&self, image_b64: &str, media_type: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "system": vision_prompt(),
            "messages": [
                {"role": "user", "content": [
                    {"type": "image", "source": {"type": "base64", "media_type": media_type, "data": image_b64}},
                    {"type": "text", "text": "Extract the knowledge graph from this image."}
                ]}
            ]
        })
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Content-Type", "application/json".into()),
            ("x-api-key", self.api_key.clone()),
            ("anthropic-version", "2023-06-01".into()),
        ]
    }

    /// Single-shot API call for one piece of content.
    fn extract_raw(&self, content: &str, file_type: &str) -> Result<SemanticExtraction> {
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

    fn parse_messages_response(&self, response: &str) -> Result<SemanticExtraction> {
        let json: serde_json::Value = serde_json::from_str(response)
            .map_err(|e| AstriaError::Graph(format!("Failed to parse Claude API response: {e}")))?;
        let text = json
            .get("content")
            .and_then(|c| c.get(0))
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        Ok(parse_extraction_text(text))
    }

    fn extract_image_raw(
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
}

// ---------------------------------------------------------------------------
// OpenAiBackend (any OpenAI-compatible endpoint)
// ---------------------------------------------------------------------------

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

    fn headers(&self) -> Vec<(&'static str, String)> {
        let mut headers = vec![("Content-Type", "application/json".to_string())];
        if let Some(key) = &self.api_key {
            headers.push(("Authorization", format!("Bearer {key}")));
        }
        headers
    }

    fn extract_raw(&self, body: serde_json::Value) -> Result<SemanticExtraction> {
        let body_str = serde_json::to_string(&body)?;
        let url = format!("{}/chat/completions", self.base_url);
        let headers = self.headers();
        let hdr: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let response = post_json(&self.agent, &url, &hdr, &body_str, "OpenAI-compatible")?;
        let json: serde_json::Value = serde_json::from_str(&response)
            .map_err(|e| AstriaError::Graph(format!("Failed to parse OpenAI response: {e}")))?;
        let text = json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        Ok(parse_extraction_text(text))
    }
}

impl SemanticBackend for OpenAiBackend {
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
}

// ---------------------------------------------------------------------------
// GeminiBackend (Google Generative Language API)
// ---------------------------------------------------------------------------

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

    fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Content-Type", "application/json".into()),
            ("x-goog-api-key", self.api_key.clone()),
        ]
    }

    pub fn build_request_body(&self, content: &str, file_type: &str) -> serde_json::Value {
        serde_json::json!({
            "system_instruction": {"parts": [{"text": system_prompt(file_type)}]},
            "contents": [{"role": "user", "parts": [{"text": content}]}],
            "generationConfig": {"maxOutputTokens": 4096}
        })
    }

    pub fn build_image_request_body(&self, image_b64: &str, media_type: &str) -> serde_json::Value {
        serde_json::json!({
            "system_instruction": {"parts": [{"text": vision_prompt()}]},
            "contents": [{"role": "user", "parts": [
                {"inline_data": {"mime_type": media_type, "data": image_b64}},
                {"text": "Extract the knowledge graph from this image."}
            ]}],
            "generationConfig": {"maxOutputTokens": 4096}
        })
    }

    fn extract_raw(&self, body: serde_json::Value) -> Result<SemanticExtraction> {
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
        let text = json
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        Ok(parse_extraction_text(text))
    }
}

impl SemanticBackend for GeminiBackend {
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
}

// ---------------------------------------------------------------------------
// Backend resolution
// ---------------------------------------------------------------------------

/// Resolve the semantic backend from the environment.
///
/// `ASTRIA_LLM_BACKEND` selects explicitly (`claude`/`anthropic`,
/// `openai`/`openai-compatible`, `gemini`); otherwise backends are
/// auto-detected from which variables are set, in the order
/// Claude → OpenAI-compatible → Gemini.
pub fn backend_from_env() -> Result<Box<dyn SemanticBackend>> {
    let explicit = astria_core::env_var("LLM_BACKEND").unwrap_or_default();
    let explicit = explicit.trim().to_lowercase();
    match explicit.as_str() {
        "claude" | "anthropic" => return Ok(Box::new(ClaudeBackend::from_env()?)),
        "openai" | "openai-compatible" | "openai_compatible" => {
            return Ok(Box::new(OpenAiBackend::from_env()?))
        }
        "gemini" | "google" => return Ok(Box::new(GeminiBackend::from_env()?)),
        "" => {}
        other => {
            return Err(AstriaError::Graph(format!(
                "unknown ASTRIA_LLM_BACKEND '{other}' (expected claude, openai, or gemini)"
            )))
        }
    }

    if astria_core::env_var("LLM_PROVIDER").is_some()
        && astria_core::env_var("LLM_BASE_URL").is_some()
    {
        return Ok(Box::new(OpenAiBackend::from_env()?));
    }
    // ASTRIA_LLM_API_KEY alone historically meant Claude; a base URL or
    // Gemini/Google key steers elsewhere.
    if astria_core::env_var("LLM_BASE_URL").is_some() {
        return Ok(Box::new(OpenAiBackend::from_env()?));
    }
    if (std::env::var("GEMINI_API_KEY").is_ok() || std::env::var("GOOGLE_API_KEY").is_ok())
        && astria_core::env_var("LLM_API_KEY").is_none()
    {
        return Ok(Box::new(GeminiBackend::from_env()?));
    }
    Ok(Box::new(ClaudeBackend::from_env()?))
}

// ---------------------------------------------------------------------------
// Public helpers
// ---------------------------------------------------------------------------

/// Image extensions routed to vision extraction.
pub fn is_image_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif")
    )
}

/// Media type for an image path (for multimodal payloads).
pub fn image_media_type(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    }
}

/// Read the given files and run semantic extraction on each using the
/// provided backend. Text files go through text extraction; image files go
/// through the backend's vision path. Unreadable/oversized files are
/// skipped silently; API failures are returned per file (not swallowed) so
/// callers can surface them. Returns `(file_path, result)` pairs to
/// preserve file provenance.
pub fn extract_semantic_for_files(
    files: &[PathBuf],
    backend: &dyn SemanticBackend,
) -> Vec<(PathBuf, Result<SemanticExtraction>)> {
    let mut results = Vec::new();
    for (i, path) in files.iter().enumerate() {
        if i > 0 {
            // Simple rate-limiting: pause between API calls to avoid hitting limits.
            std::thread::sleep(Duration::from_millis(500));
        }
        if is_image_file(path) {
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            if bytes.len() > MAX_IMAGE_BYTES {
                continue;
            }
            results.push((
                path.clone(),
                backend.extract_semantic_from_image(&bytes, image_media_type(path)),
            ));
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown");
        results.push((path.clone(), backend.extract_semantic(&content, ext)));
    }
    results
}

/// Default concurrent workers for batch semantic extraction.
pub const DEFAULT_CONCURRENCY: usize = 4;
/// Hard ceiling — LLM rate limits punish more than this.
pub const MAX_CONCURRENCY: usize = 8;

/// Resolve the worker count from `ASTRIA_LLM_CONCURRENCY` (1..=8),
/// defaulting to 4.
pub fn concurrency_from_env() -> usize {
    astria_core::env_var("LLM_CONCURRENCY")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|v| v.clamp(1, MAX_CONCURRENCY))
        .unwrap_or(DEFAULT_CONCURRENCY)
}

/// Run semantic extraction over a batch of files with a bounded worker
/// pool. `backend_factory` is called once per worker so each thread owns
/// its backend (`SemanticBackend` is not `Sync`). Results are returned in
/// the original file order; unreadable/oversized files are skipped exactly
/// as in `extract_semantic_for_files`. With `workers <= 1` this is a
/// single-threaded call and the factory is invoked once.
pub fn extract_semantic_for_files_parallel<F>(
    files: &[PathBuf],
    backend_factory: F,
    workers: usize,
) -> Vec<(PathBuf, Result<SemanticExtraction>)>
where
    F: Fn() -> Result<Box<dyn SemanticBackend>> + Sync,
{
    if files.is_empty() {
        return Vec::new();
    }
    let workers = workers.clamp(1, MAX_CONCURRENCY).min(files.len());

    if workers <= 1 {
        return match backend_factory() {
            Ok(backend) => extract_semantic_for_files(files, backend.as_ref()),
            Err(e) => files
                .iter()
                .map(|f| (f.clone(), Err(AstriaError::Graph(e.to_string()))))
                .collect(),
        };
    }

    // Contiguous chunks, one per worker.
    let chunk_size = files.len().div_ceil(workers);
    let chunks: Vec<&[PathBuf]> = files.chunks(chunk_size).collect();

    let results: std::sync::Mutex<Vec<(PathBuf, Result<SemanticExtraction>)>> =
        std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for chunk in chunks {
            let results = &results;
            let backend_factory = &backend_factory;
            scope.spawn(move || {
                let backend = match backend_factory() {
                    Ok(b) => b,
                    Err(e) => {
                        // Backend unavailable on this worker: fail the chunk's
                        // files explicitly instead of dropping them.
                        let message = e.to_string();
                        let mut guard = results.lock().unwrap();
                        for f in chunk {
                            guard.push((f.clone(), Err(AstriaError::Graph(message.clone()))));
                        }
                        return;
                    }
                };
                // Extract WITHOUT holding the lock — the whole point is
                // concurrent API calls. Only result collection is locked.
                let local = extract_semantic_for_files(chunk, backend.as_ref());
                let mut guard = results.lock().unwrap();
                guard.extend(local);
            });
        }
    });

    // Restore deterministic (original file) order.
    let mut order: HashMap<&PathBuf, usize> = HashMap::with_capacity(files.len());
    for (i, f) in files.iter().enumerate() {
        order.insert(f, i);
    }
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(path, _)| order.get(path).copied().unwrap_or(usize::MAX));
    results
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_backend_returns_empty() {
        let backend = NoopBackend;
        let result = backend.extract_semantic("content", "txt").unwrap();
        assert!(result.nodes.is_empty());
        assert!(result.edges.is_empty());
    }

    #[test]
    fn noop_backend_has_no_vision() {
        assert!(NoopBackend
            .extract_semantic_from_image(&[], "image/png")
            .is_err());
    }

    // -- Claude payloads --

    #[test]
    fn claude_request_body_structure() {
        let backend = ClaudeBackend::new("key".into(), "claude-sonnet-4-20250514".into());
        let body = backend.build_request_body("hello", "rust");
        assert_eq!(body["model"], "claude-sonnet-4-20250514");
        assert_eq!(body["messages"][0]["content"], "hello");
        assert!(body["system"].as_str().unwrap().contains("rust"));
    }

    #[test]
    fn claude_image_payload_structure() {
        let backend = ClaudeBackend::new("key".into(), "m".into());
        let body = backend.build_image_request_body("QUJD", "image/png");
        let block = &body["messages"][0]["content"][0];
        assert_eq!(block["type"], "image");
        assert_eq!(block["source"]["media_type"], "image/png");
        assert_eq!(block["source"]["data"], "QUJD");
        assert_eq!(body["messages"][0]["content"][1]["type"], "text");
    }

    // -- OpenAI-compatible payloads --

    #[test]
    fn openai_request_body_structure() {
        let backend = OpenAiBackend::new(
            Some("k".into()),
            "https://api.openai.com/v1".into(),
            "gpt-4o-mini".into(),
        );
        let body = backend.build_request_body("hello", "python");
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "hello");
    }

    #[test]
    fn openai_image_payload_uses_data_url() {
        let backend = OpenAiBackend::new(Some("k".into()), "https://x/v1".into(), "m".into());
        let body = backend.build_image_request_body("QUJD", "image/jpeg");
        let part = &body["messages"][1]["content"][1];
        assert_eq!(part["type"], "image_url");
        assert_eq!(part["image_url"]["url"], "data:image/jpeg;base64,QUJD");
    }

    #[test]
    fn openai_local_endpoint_needs_no_key() {
        let backend = OpenAiBackend::new(None, "http://localhost:11434/v1".into(), "llama3".into());
        // local model, no Authorization header expected
        let body = backend.build_request_body("x", "txt");
        assert_eq!(body["model"], "llama3");
    }

    #[test]
    fn local_base_url_detection() {
        assert!(is_local_base_url("http://localhost:11434/v1"));
        assert!(is_local_base_url("http://127.0.0.1:8000"));
        assert!(is_local_base_url("http://[::1]:11434/v1"));
        assert!(is_local_base_url("http://user@localhost/v1"));
        assert!(!is_local_base_url("http://api.example.com/v1"));
        assert!(!is_local_base_url("http://localhost.attacker.com"));
        assert!(!is_local_base_url("https://api.openai.com/v1"));
    }

    // -- Gemini payloads --

    #[test]
    fn gemini_request_body_structure() {
        let backend = GeminiBackend::new("k".into(), "gemini-2.0-flash".into());
        let body = backend.build_request_body("hello", "markdown");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "hello");
        assert!(body["system_instruction"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("markdown"));
        assert!(backend.url().contains("gemini-2.0-flash:generateContent"));
    }

    #[test]
    fn gemini_url_does_not_contain_api_key() {
        let backend = GeminiBackend::new("supersecret".into(), "gemini-2.0-flash".into());
        assert!(!backend.url().contains("supersecret"));
        let headers = backend.headers();
        assert!(headers
            .iter()
            .any(|(k, v)| *k == "x-goog-api-key" && v == "supersecret"));
    }

    #[test]
    fn gemini_image_payload_uses_inline_data() {
        let backend = GeminiBackend::new("k".into(), "m".into());
        let body = backend.build_image_request_body("QUJD", "image/webp");
        assert_eq!(
            body["contents"][0]["parts"][0]["inline_data"]["mime_type"],
            "image/webp"
        );
        assert_eq!(
            body["contents"][0]["parts"][0]["inline_data"]["data"],
            "QUJD"
        );
    }

    // -- Resolution --

    /// Env-mutating tests must not run concurrently.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn backend_resolution_unknown_name_errors() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ASTRIA_LLM_BACKEND", "gpt9");
        let err = match backend_from_env() {
            Err(e) => e,
            Ok(_) => panic!("expected error for unknown backend name"),
        };
        assert!(err.to_string().contains("unknown ASTRIA_LLM_BACKEND"));
        std::env::remove_var("ASTRIA_LLM_BACKEND");
    }

    #[test]
    fn backend_resolution_explicit_openai() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ASTRIA_LLM_BACKEND", "openai");
        std::env::set_var("OPENAI_API_KEY", "test");
        // must not error (constructs the backend without network access)
        assert!(backend_from_env().is_ok());
        std::env::remove_var("ASTRIA_LLM_BACKEND");
        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn legacy_graphify_env_still_resolves() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("ASTRIA_LLM_BACKEND");
        std::env::set_var("GRAPHIFY_LLM_BACKEND", "openai");
        std::env::set_var("OPENAI_API_KEY", "test");
        assert!(backend_from_env().is_ok());
        std::env::remove_var("GRAPHIFY_LLM_BACKEND");
        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn astria_env_wins_over_legacy_graphify() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ASTRIA_LLM_MODEL", "new");
        std::env::set_var("GRAPHIFY_LLM_MODEL", "old");
        assert_eq!(astria_core::env_var("LLM_MODEL").as_deref(), Some("new"));
        std::env::remove_var("ASTRIA_LLM_MODEL");
        assert_eq!(astria_core::env_var("LLM_MODEL").as_deref(), Some("old"));
        std::env::remove_var("GRAPHIFY_LLM_MODEL");
        assert_eq!(astria_core::env_var("LLM_MODEL"), None);
    }

    // -- Parsing --

    #[test]
    fn parse_extraction_tolerates_prose_around_json() {
        let text = "Here you go:\n{\"nodes\":[{\"id\":\"a\",\"label\":\"A\",\"summary\":\"s\",\"node_type\":\"concept\"}],\"edges\":[]}\nDone.";
        let parsed = parse_extraction_text(text);
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].id, "a");
    }

    #[test]
    fn parse_extraction_empty_text() {
        assert!(parse_extraction_text("").nodes.is_empty());
        assert!(parse_extraction_text("no json here").nodes.is_empty());
    }

    // -- Output validation --

    #[test]
    fn sanitize_clamps_enums_and_drops_dangling_edges() {
        let ext = SemanticExtraction {
            nodes: vec![
                SemanticNode {
                    id: " a ".into(),
                    label: "A".into(),
                    summary: String::new(),
                    node_type: "turbine".into(),
                },
                SemanticNode {
                    id: String::new(),
                    label: "empty id".into(),
                    summary: String::new(),
                    node_type: "concept".into(),
                },
                SemanticNode {
                    id: "b".into(),
                    label: "B".into(),
                    summary: String::new(),
                    node_type: "entity".into(),
                },
            ],
            edges: vec![
                SemanticEdge {
                    source: "a".into(),
                    target: "ghost".into(),
                    relation: "uses".into(),
                },
                SemanticEdge {
                    source: "a".into(),
                    target: "b".into(),
                    relation: "forks".into(),
                },
                SemanticEdge {
                    source: "a".into(),
                    target: "a".into(),
                    relation: "uses".into(),
                },
            ],
        };
        let clean = sanitize_extraction(ext);
        assert_eq!(clean.nodes.len(), 2, "empty-id node dropped");
        assert_eq!(clean.nodes[0].id, "a");
        assert_eq!(clean.nodes[0].node_type, "concept", "unknown type clamped");
        assert_eq!(clean.edges.len(), 1, "dangling and self-loop edges dropped");
        assert_eq!(
            clean.edges[0].relation, "relates_to",
            "unknown relation clamped"
        );
    }

    // -- Chunking --

    #[test]
    fn split_chunks_respects_line_boundaries() {
        let content = "line\n".repeat(10_000); // 50k chars > MAX_CHUNK_CHARS
        let chunks = split_chunks(&content);
        assert!(chunks.len() > 1);
        let rejoined: String = chunks.concat();
        assert_eq!(rejoined, content, "chunking must not lose content");
        for chunk in &chunks {
            assert!(chunk.ends_with('\n'));
        }
    }

    #[test]
    fn short_content_is_a_single_chunk() {
        assert_eq!(split_chunks("hello world").len(), 1);
    }

    #[test]
    fn merge_dedupes_nodes_and_keeps_cross_chunk_edges() {
        let part1 = SemanticExtraction {
            nodes: vec![SemanticNode {
                id: "a".into(),
                label: "A".into(),
                summary: String::new(),
                node_type: "concept".into(),
            }],
            edges: vec![],
        };
        let part2 = SemanticExtraction {
            nodes: vec![
                SemanticNode {
                    id: "a".into(),
                    label: "A again".into(),
                    summary: String::new(),
                    node_type: "concept".into(),
                },
                SemanticNode {
                    id: "b".into(),
                    label: "B".into(),
                    summary: String::new(),
                    node_type: "concept".into(),
                },
            ],
            edges: vec![SemanticEdge {
                source: "b".into(),
                target: "a".into(),
                relation: "uses".into(),
            }],
        };
        let merged = sanitize_extraction(merge_extractions(vec![part1, part2]));
        assert_eq!(merged.nodes.len(), 2, "duplicate id across chunks merged");
        assert_eq!(
            merged.edges.len(),
            1,
            "cross-chunk reference survives merge"
        );
    }

    // -- Parallel batch --

    #[test]
    fn parallel_extraction_preserves_order_and_results() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        for i in 0..9 {
            let p = dir.path().join(format!("f{i}.txt"));
            std::fs::write(&p, format!("content {i}")).unwrap();
            files.push(p);
        }
        let factory = || Ok(Box::new(NoopBackend) as Box<dyn SemanticBackend>);
        let results = extract_semantic_for_files_parallel(&files, factory, 3);
        assert_eq!(results.len(), 9);
        // Original order preserved
        for (i, (path, result)) in results.iter().enumerate() {
            assert_eq!(path, &files[i]);
            assert!(result.is_ok(), "NoopBackend never fails");
        }
    }

    #[test]
    fn parallel_extraction_reports_backend_failure_per_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        for i in 0..4 {
            let p = dir.path().join(format!("f{i}.txt"));
            std::fs::write(&p, format!("content {i}")).unwrap();
            files.push(p);
        }
        let factory =
            || -> Result<Box<dyn SemanticBackend>> { Err(AstriaError::Graph("no key".into())) };
        let results = extract_semantic_for_files_parallel(&files, factory, 2);
        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|(_, r)| r.is_err()));
    }

    #[test]
    fn concurrency_env_parsing_clamped() {
        // Guard against concurrent env access from other tests
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = LOCK.lock().unwrap();
        std::env::remove_var("ASTRIA_LLM_CONCURRENCY");
        std::env::remove_var("GRAPHIFY_LLM_CONCURRENCY");
        assert_eq!(concurrency_from_env(), DEFAULT_CONCURRENCY);
        std::env::set_var("ASTRIA_LLM_CONCURRENCY", "100");
        assert_eq!(concurrency_from_env(), MAX_CONCURRENCY);
        std::env::set_var("ASTRIA_LLM_CONCURRENCY", "0");
        assert_eq!(concurrency_from_env(), 1);
        std::env::set_var("ASTRIA_LLM_CONCURRENCY", "bogus");
        assert_eq!(concurrency_from_env(), DEFAULT_CONCURRENCY);
        std::env::remove_var("ASTRIA_LLM_CONCURRENCY");
    }

    // -- File routing --

    #[test]
    fn image_files_detected() {
        assert!(is_image_file(std::path::Path::new("photo.PNG")));
        assert!(is_image_file(std::path::Path::new("d.webp")));
        assert!(!is_image_file(std::path::Path::new("doc.md")));
        assert_eq!(image_media_type(std::path::Path::new("x.png")), "image/png");
        assert_eq!(
            image_media_type(std::path::Path::new("x.jpg")),
            "image/jpeg"
        );
    }

    #[test]
    fn extraction_roundtrip() {
        let ext = SemanticExtraction {
            nodes: vec![SemanticNode {
                id: "graph_algo".into(),
                label: "Graph Algorithm".into(),
                summary: "Traversal".into(),
                node_type: "concept".into(),
            }],
            edges: vec![SemanticEdge {
                source: "graph_algo".into(),
                target: "bfs".into(),
                relation: "contains".into(),
            }],
        };
        let json = serde_json::to_string(&ext).unwrap();
        let parsed: SemanticExtraction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.nodes[0].id, "graph_algo");
        assert_eq!(parsed.edges[0].relation, "contains");
    }
}
