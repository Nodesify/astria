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

// NoopBackend
// ---------------------------------------------------------------------------

/// A no-op backend that always returns empty extractions.
pub struct NoopBackend;

impl SemanticBackend for NoopBackend {
    fn extract_semantic(&self, _content: &str, _file_type: &str) -> Result<SemanticExtraction> {
        Ok(SemanticExtraction::empty())
    }
}
pub mod enrichment;
pub mod jev;

/// Env-mutating tests (backend resolution, config parsing) must not run
/// concurrently — shared by every test module in this crate.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Maximum image size sent to vision endpoints (5 MB, matching upstream).
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

/// Long content is extracted in chunks of this many characters (about 6k
/// tokens) instead of one oversized request that would blow the context
/// window or silently truncate.
const MAX_CHUNK_CHARS: usize = 24_000;
/// Default API-call budget for one file: at most this many chunks are
/// extracted. Overridable via `ASTRIA_LLM_MAX_CHUNKS` (1..=64) for
/// oversized generated files; content beyond the cap is a hard error.
const MAX_CHUNKS: usize = 8;
/// Ceiling for a user-configured chunk cap.
const MAX_CHUNKS_CEILING: usize = 64;

/// Effective per-file chunk cap (`ASTRIA_LLM_MAX_CHUNKS`, clamped).
pub fn max_chunks() -> usize {
    astria_core::env_var("LLM_MAX_CHUNKS")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|v| v.clamp(1, MAX_CHUNKS_CEILING))
        .unwrap_or(MAX_CHUNKS)
}
/// Ceiling for a server-supplied Retry-After (seconds) before falling back
/// to the default backoff schedule.
const MAX_RETRY_AFTER_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

// Domain modules split out of lib.rs: shared types/protocol helpers, then
// one file per extraction backend. The public API surface is unchanged.
mod backend_azure;
mod backend_bedrock;
mod backend_claude;
mod backend_gemini;
mod backend_jev;
mod backend_openai;
mod chunking;
mod http;
mod prompt;
mod sigv4;
mod types;

pub use backend_azure::*;
pub use backend_bedrock::*;
pub use backend_claude::*;
pub use backend_gemini::*;
pub use backend_jev::*;
pub use backend_openai::*;
pub(crate) use chunking::*;
pub(crate) use http::*;
pub(crate) use prompt::*;
pub use types::*;
/// The judge layer selected via `--judge` / `ASTRIA_LLM_JUDGE`, if any.
/// Judges wrap an engine backend (see `backend_from_env`); they are never
/// backends themselves because Jev cannot generate extractions.
pub fn judge_from_env() -> Option<String> {
    std::env::var("ASTRIA_LLM_JUDGE")
        .ok()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
}

/// Resolve the semantic backend from the environment.
///
/// Network enrichment requires explicit `ASTRIA_LLM_BACKEND` selection.
/// Credentials configure a selected backend; they never activate one.
/// When `ASTRIA_LLM_JUDGE` names a judge layer, the resolved engine is
/// wrapped: the engine still generates, the judge re-judges.
pub fn backend_from_env() -> Result<Box<dyn SemanticBackend>> {
    let explicit = std::env::var("ASTRIA_LLM_BACKEND").unwrap_or_default();
    let explicit = explicit.trim().to_lowercase();
    let engine: Box<dyn SemanticBackend> = match explicit.as_str() {
                "claude" | "anthropic" => Box::new(ClaudeBackend::from_env()?),
        "openai" | "openai-compatible" | "openai_compatible" => {
            Box::new(OpenAiBackend::from_env()?)
        }
        "kimi" | "moonshot" => Box::new(OpenAiBackend::kimi_from_env()?),
        "azure" | "azure-openai" | "azure_openai" => Box::new(AzureOpenAiBackend::from_env()?),
        "bedrock" | "aws" => Box::new(BedrockBackend::from_env()?),
        "gemini" | "google" => Box::new(GeminiBackend::from_env()?),
        "jev" | "typesafe" => {
            return Err(AstriaError::Graph(
                "Jev is a judge, not a generator — it cannot produce extractions on its own; \
                 select an engine with --backend/ASTRIA_LLM_BACKEND and add --judge jev \
                 (ASTRIA_LLM_JUDGE=jev)"
                    .into(),
            ))
        }
        "" | "none" => {
            return Err(AstriaError::Graph(
                "semantic enrichment is disabled; select --backend claude|openai|azure|bedrock|kimi|gemini or ASTRIA_LLM_BACKEND explicitly".into(),
            ))
        }
        other => {
            return Err(AstriaError::Graph(format!(
                "unknown ASTRIA_LLM_BACKEND '{other}' (expected claude, openai, azure, bedrock, kimi, or gemini)"
            )))
        }
    };
    match judge_from_env() {
        None => Ok(engine),
        Some(name) if name == "jev" => Ok(Box::new(JevJudgeBackend::new(
            engine,
            jev::JevConfig::from_env()?,
        ))),
        Some(other) => Err(AstriaError::Graph(format!(
            "unknown ASTRIA_LLM_JUDGE '{other}' (expected jev)"
        ))),
    }
}

/// Includes effective backend/model/endpoint and prompt/chunking inputs, never
/// credentials. Callers hash this material with the stage's actual inputs.
/// The EFFECTIVE chunk cap participates: raising ASTRIA_LLM_MAX_CHUNKS must
/// invalidate cached results produced under the tighter cap.
pub fn cache_configuration(backend: &dyn SemanticBackend) -> String {
    format!(
        "semantic-v2\n{}\n{MAX_CHUNK_CHARS}:{}\n{}\n{}\n{}",
        backend.cache_identity(),
        max_chunks(),
        system_prompt("code"),
        system_prompt("document"),
        vision_prompt()
    )
}

pub fn enrichment_enabled() -> bool {
    std::env::var("ASTRIA_LLM_BACKEND")
        .map(|value| !value.trim().is_empty() && !value.trim().eq_ignore_ascii_case("none"))
        .unwrap_or(false)
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

/// Where a file's LLM-facing text comes from.
#[derive(Debug, Clone)]
pub enum SourceContent {
    /// Read the file from disk (text source files; PDFs re-parsed via
    /// astria-pdf).
    FromDisk,
    /// Pre-extracted content from the document layer — binary formats
    /// (office documents, workspace exports, media transcripts, PDFs)
    /// converted once; the LLM sees exactly the normalized content
    /// extraction saw, never raw bytes reinterpreted as UTF-8.
    Extracted(String),
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
    let sources: Vec<(PathBuf, SourceContent)> = files
        .iter()
        .map(|f| (f.clone(), SourceContent::FromDisk))
        .collect();
    extract_semantic_with_sources(&sources, backend)
}

/// The extraction loop with explicit per-file content: binary formats
/// arrive as their document-layer text instead of being read raw.
pub fn extract_semantic_with_sources(
    sources: &[(PathBuf, SourceContent)],
    backend: &dyn SemanticBackend,
) -> Vec<(PathBuf, Result<SemanticExtraction>)> {
    let mut results = Vec::new();
    for (i, (path, source)) in sources.iter().enumerate() {
        if i > 0 {
            // Simple rate-limiting: pause between API calls to avoid hitting limits.
            std::thread::sleep(Duration::from_millis(500));
        }
        // MCP server configurations carry literal credentials; they are
        // ingested by the deterministic manifest extractor only. No call
        // path may read these raw and hand them to a backend — checked
        // before any reservation or content read.
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(astria_core::is_mcp_config_filename)
        {
            results.push((
                path.clone(),
                Err(AstriaError::Graph(format!(
                    "refusing semantic extraction on MCP config {}: credentials never leave the machine",
                    path.display()
                ))),
            ));
            continue;
        }
        if is_image_file(path) {
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(error) => {
                    results.push((path.clone(), Err(error.into())));
                    continue;
                }
            };
            if bytes.len() > MAX_IMAGE_BYTES {
                results.push((
                    path.clone(),
                    Err(AstriaError::Graph(format!(
                        "image exceeds the {MAX_IMAGE_BYTES}-byte vision limit: {}",
                        path.display()
                    ))),
                ));
                continue;
            }
            // One billable vision request: claim its reservation (image
            // bytes as a conservative token estimate plus the full
            // extraction output allowance) before it flies. The guard
            // releases on any exit path.
            match enrichment::reserve_budget(bytes.len() / 4, enrichment::MAX_OUTPUT_TOKENS_EXTRACT)
            {
                Err(e) => results.push((path.clone(), Err(e))),
                Ok(_reservation) => {
                    let vision =
                        backend.extract_semantic_from_image(&bytes, image_media_type(path));
                    results.push((path.clone(), vision));
                }
            }
            continue;
        }
        let content = match source {
            SourceContent::Extracted(text) => Ok(text.clone()),
            SourceContent::FromDisk => {
                if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
                {
                    astria_pdf::extract_text(path)
                } else {
                    std::fs::read_to_string(path).map_err(AstriaError::from)
                }
            }
        };
        let content = match content {
            Ok(c) => c,
            Err(error) => {
                results.push((path.clone(), Err(error)));
                continue;
            }
        };
        // No file-level reservation here: the chunked extraction driver
        // claims one per chunk REQUEST, which is the actual billable unit —
        // a file-level claim would both under-count multi-chunk files
        // (several output allowances) and double-count single-chunk ones.
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown");
        let extraction = backend.extract_semantic(&content, ext);
        results.push((path.clone(), extraction));
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
/// Run a batch semantic extraction over a bounded worker pool.
/// `backend_factory` is called once per worker so each thread owns its
/// backend (`SemanticBackend` is not `Sync`). Results are returned in the
/// original file order. `sources` maps paths to their LLM-facing content
/// (see [`SourceContent`]); files absent from the map are read from disk.
pub fn extract_semantic_for_files_parallel_with_content<F>(
    files: &[PathBuf],
    backend_factory: F,
    sources: std::sync::Arc<HashMap<PathBuf, SourceContent>>,
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
            Ok(backend) => {
                let list: Vec<(PathBuf, SourceContent)> = files
                    .iter()
                    .map(|f| {
                        let content = sources.get(f).cloned().unwrap_or(SourceContent::FromDisk);
                        (f.clone(), content)
                    })
                    .collect();
                extract_semantic_with_sources(&list, backend.as_ref())
            }
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
            let sources = &sources;
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
                let list: Vec<(PathBuf, SourceContent)> = chunk
                    .iter()
                    .map(|f| {
                        let content = sources.get(f).cloned().unwrap_or(SourceContent::FromDisk);
                        (f.clone(), content)
                    })
                    .collect();
                let local = extract_semantic_with_sources(&list, backend.as_ref());
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

/// Run a batch semantic extraction over a bounded worker pool, reading all
/// files from disk. See `extract_semantic_for_files_parallel_with_content`
/// for the pre-extracted-content variant used by the pipeline.
pub fn extract_semantic_for_files_parallel<F>(
    files: &[PathBuf],
    backend_factory: F,
    workers: usize,
) -> Vec<(PathBuf, Result<SemanticExtraction>)>
where
    F: Fn() -> Result<Box<dyn SemanticBackend>> + Sync,
{
    extract_semantic_for_files_parallel_with_content(
        files,
        backend_factory,
        std::sync::Arc::new(HashMap::new()),
        workers,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_configs_are_never_read_raw() {
        // MCP configs embed credentials; the batch extractor must refuse
        // them before any read or backend call, whatever the caller passes.
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join(".mcp.json");
        std::fs::write(
            &config,
            r#"{"mcpServers":{"x":{"env":{"TOKEN":"hunter2"}}}}"#,
        )
        .unwrap();
        let results = extract_semantic_for_files(&[config.clone()], &NoopBackend);
        let (path, result) = &results[0];
        assert_eq!(path, &config);
        let err = result.as_ref().unwrap_err().to_string();
        assert!(
            err.contains("credentials never leave the machine"),
            "got: {err}"
        );
        // The backend must not have received the file's content either: a
        // NoopBackend "succeeds" on anything it is handed, so an error here
        // proves the refusal fired first.
    }

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
    fn bedrock_request_body_uses_converse_format() {
        let backend = BedrockBackend::new(
            "us-east-1".into(),
            "anthropic.claude-3-5-sonnet-20241022-v2:0".into(),
            "AK".into(),
            "SK".into(),
            None,
        );
        let body = backend.build_request_body("hello", "rust");
        assert!(body["system"][0]["text"].as_str().unwrap().contains("rust"));
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["text"], "hello");
        assert_eq!(body["inferenceConfig"]["maxTokens"], 4096);
        // Colon-bearing model ids must be percent-encoded for the URL path.
        assert!(backend
            .url()
            .contains("/model/anthropic.claude-3-5-sonnet-20241022-v2%3A0/converse"));
        assert!(
            !backend.url().contains("AK"),
            "credentials never appear in the URL"
        );
    }

    #[test]
    fn bedrock_image_body_uses_converse_image_block() {
        let backend = BedrockBackend::new(
            "eu-west-1".into(),
            "m".into(),
            "AK".into(),
            "SK".into(),
            Some("TOKEN".into()),
        );
        let body = backend.build_image_request_body("QUJD", "image/webp");
        let block = &body["messages"][0]["content"][0];
        assert_eq!(block["image"]["format"], "webp");
        assert_eq!(block["image"]["source"]["bytes"], "QUJD");
    }

    #[test]
    fn kimi_resolution_builds_moonshot_backend() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ASTRIA_LLM_BACKEND", "kimi");
        std::env::set_var("MOONSHOT_API_KEY", "sk-test");
        std::env::remove_var("ASTRIA_LLM_MODEL");
        assert!(backend_from_env().is_ok());
        std::env::remove_var("ASTRIA_LLM_BACKEND");
        std::env::remove_var("MOONSHOT_API_KEY");
    }

    #[test]
    fn kimi_resolution_without_key_errors_helpfully() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ASTRIA_LLM_BACKEND", "kimi");
        for var in [
            "MOONSHOT_API_KEY",
            "KIMI_API_KEY",
            "ASTRIA_LLM_API_KEY",
            "OPENAI_API_KEY",
        ] {
            std::env::remove_var(var);
        }
        let err = backend_from_env().err().expect("missing key must error");
        assert!(err.to_string().contains("MOONSHOT_API_KEY"));
        std::env::remove_var("ASTRIA_LLM_BACKEND");
    }

    #[test]
    fn azure_resolution_builds_backend() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ASTRIA_LLM_BACKEND", "azure");
        std::env::set_var("ASTRIA_AZURE_ENDPOINT", "https://res.openai.azure.com");
        std::env::set_var("ASTRIA_AZURE_DEPLOYMENT", "gpt4o");
        std::env::set_var("ASTRIA_AZURE_API_KEY", "key");
        assert!(backend_from_env().is_ok());
        for var in [
            "ASTRIA_LLM_BACKEND",
            "ASTRIA_AZURE_ENDPOINT",
            "ASTRIA_AZURE_DEPLOYMENT",
            "ASTRIA_AZURE_API_KEY",
        ] {
            std::env::remove_var(var);
        }
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
    fn no_backend_selection_leaves_enrichment_disabled() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("ASTRIA_LLM_BACKEND");
        std::env::set_var("OPENAI_API_KEY", "test");
        let err = backend_from_env()
            .err()
            .expect("explicit backend selection required");
        assert!(err.to_string().contains("semantic enrichment is disabled"));
        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn env_var_reads_only_current_spelling() {
        // There is no legacy fallback: only ASTRIA_* names are read, and an
        // empty ASTRIA_ value counts as unset.
        std::env::set_var("ASTRIA_LLM_MODEL", "new");
        assert_eq!(astria_core::env_var("LLM_MODEL").as_deref(), Some("new"));
        std::env::set_var("ASTRIA_LLM_MODEL", "");
        assert_eq!(astria_core::env_var("LLM_MODEL"), None);
        std::env::remove_var("ASTRIA_LLM_MODEL");
    }

    // -- Jev judge layer --

    #[test]
    fn backend_resolution_engine_with_jev_judge() {
        let _guard = crate::ENV_LOCK.lock().unwrap();
        std::env::set_var("ASTRIA_LLM_BACKEND", "openai");
        std::env::set_var("ASTRIA_LLM_JUDGE", "jev");
        std::env::set_var("OPENAI_API_KEY", "k");
        std::env::set_var("TYPESAFE_API_KEY", "k");
        assert!(backend_from_env().is_ok());
        std::env::remove_var("ASTRIA_LLM_BACKEND");
        std::env::remove_var("ASTRIA_LLM_JUDGE");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("TYPESAFE_API_KEY");
    }

    #[test]
    fn backend_resolution_jev_backend_redirects_to_judge() {
        let _guard = crate::ENV_LOCK.lock().unwrap();
        std::env::set_var("ASTRIA_LLM_BACKEND", "jev");
        std::env::set_var("TYPESAFE_API_KEY", "k");
        let err = backend_from_env().err().expect("jev backend must redirect");
        assert!(err.to_string().contains("judge, not a generator"));
        std::env::remove_var("ASTRIA_LLM_BACKEND");
        std::env::remove_var("TYPESAFE_API_KEY");
    }

    #[test]
    fn backend_resolution_unknown_judge_errors() {
        let _guard = crate::ENV_LOCK.lock().unwrap();
        std::env::set_var("ASTRIA_LLM_BACKEND", "openai");
        std::env::set_var("ASTRIA_LLM_JUDGE", "llama");
        std::env::set_var("OPENAI_API_KEY", "k");
        let err = backend_from_env().err().expect("unknown judge must error");
        assert!(err.to_string().contains("unknown ASTRIA_LLM_JUDGE"));
        std::env::remove_var("ASTRIA_LLM_BACKEND");
        std::env::remove_var("ASTRIA_LLM_JUDGE");
        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn jev_judge_cache_identity_includes_engine_and_config() {
        let config = jev::JevConfig {
            api_key: "k".into(),
            model: "jev-1.13.0".into(),
            endpoint: "https://api.typesafe.ai/v1/systemone".into(),
            verify_enabled: true,
            min_edge_probability: 0.4,
            gate_enabled: true,
            gate_max_bytes: 1024,
            gate_drop_threshold: 0.4,
            gate_batch: 50,
        };
        let identity = JevJudgeBackend::new(Box::new(NoopBackend), config.clone()).cache_identity();
        assert!(identity.contains("jev:"), "engine identity included");
        assert!(identity.contains("NoopBackend"));
        assert!(identity.contains("model=jev-1.13.0"));
        assert!(identity.contains("gate_bytes=1024"));
        let mut toggled = config.clone();
        toggled.verify_enabled = false;
        let identity2 = JevJudgeBackend::new(Box::new(NoopBackend), toggled).cache_identity();
        assert_ne!(identity, identity2, "toggling verify invalidates the cache");
    }

    #[test]
    fn default_gate_and_rank_keep_everything() {
        let backend = NoopBackend;
        let files = vec![PathBuf::from("a.rs"), PathBuf::from("b.py")];
        assert_eq!(backend.gate_files(&files), files);
        let questions = vec!["q1".to_string(), "q2".to_string()];
        assert_eq!(backend.rank_questions(&questions), vec![0, 1]);
    }

    // -- Parsing --

    #[test]
    fn parse_extraction_tolerates_prose_around_json() {
        let text = "Here you go:\n{\"nodes\":[{\"id\":\"a\",\"label\":\"A\",\"summary\":\"s\",\"node_type\":\"concept\"}],\"edges\":[]}\nDone.";
        let parsed = parse_extraction_text(text).unwrap();
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].id, "a");
    }

    #[test]
    fn intentional_empty_reply_is_ok_but_garbage_is_an_error() {
        // A schema-valid empty result is a legitimate, cacheable success.
        let empty = parse_extraction_text("{\"nodes\":[],\"edges\":[]}").unwrap();
        assert!(empty.nodes.is_empty());
        // Unusable replies (empty text, no JSON) must be errors so they are
        // retried instead of cached as successful empty extractions.
        assert!(parse_extraction_text("").is_err());
        assert!(parse_extraction_text("no json here").is_err());
        assert!(parse_extraction_text("{\"nodes\": [trunc").is_err());
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
                    confidence_score: None,
                },
                SemanticEdge {
                    source: "a".into(),
                    target: "b".into(),
                    relation: "forks".into(),
                    confidence_score: None,
                },
                SemanticEdge {
                    source: "a".into(),
                    target: "a".into(),
                    relation: "uses".into(),
                    confidence_score: None,
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
                confidence_score: None,
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
                confidence_score: None,
            }],
        };
        let json = serde_json::to_string(&ext).unwrap();
        let parsed: SemanticExtraction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.nodes[0].id, "graph_algo");
        assert_eq!(parsed.edges[0].relation, "contains");
    }
}
