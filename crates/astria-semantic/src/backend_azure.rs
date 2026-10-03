//! Azure OpenAI backend — first-class `api-key` auth against an
//! `openai.azure.com` deployment.
//!
//! Azure's chat-completions wire format is OpenAI-compatible, so the
//! request/response handling is reused from `OpenAiBackend`; what Azure
//! cannot share is authentication: requests carry an `api-key` header (not
//! a Bearer token) and the URL routes through a resource-specific
//! deployment path with a mandatory `api-version` query parameter.
#![allow(unused_imports)]

use super::*;
use astria_core::AstriaError;
use astria_core::Result;
use base64::Engine as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub struct AzureOpenAiBackend {
    inner: OpenAiBackend,
    endpoint: String,
    deployment: String,
    api_version: String,
}

impl AzureOpenAiBackend {
    /// - `ASTRIA_AZURE_ENDPOINT` (or `AZURE_OPENAI_ENDPOINT`) — e.g.
    ///   `https://my-resource.openai.azure.com`.
    /// - `ASTRIA_AZURE_DEPLOYMENT` (or `AZURE_OPENAI_DEPLOYMENT_NAME`) —
    ///   the deployment name hosting the chat model.
    /// - `ASTRIA_AZURE_API_KEY` (or `AZURE_OPENAI_API_KEY`) — the resource key.
    /// - `ASTRIA_AZURE_API_VERSION` — defaults to `2024-10-21`.
    /// - `ASTRIA_LLM_MODEL` — informational (cache identity); Azure routes by
    ///   deployment, not model name.
    pub fn from_env() -> Result<Self> {
        let endpoint = astria_core::env_var("AZURE_ENDPOINT")
            .or_else(|| std::env::var("AZURE_OPENAI_ENDPOINT").ok())
            .map(|e| e.trim_end_matches('/').to_string())
            .filter(|e| !e.is_empty())
            .ok_or_else(|| {
                AstriaError::Graph(
                    "no Azure endpoint: set ASTRIA_AZURE_ENDPOINT \
                     (e.g. https://my-resource.openai.azure.com)"
                        .into(),
                )
            })?;
        let deployment = astria_core::env_var("AZURE_DEPLOYMENT")
            .or_else(|| std::env::var("AZURE_OPENAI_DEPLOYMENT_NAME").ok())
            .filter(|d| !d.trim().is_empty())
            .ok_or_else(|| {
                AstriaError::Graph(
                    "no Azure deployment: set ASTRIA_AZURE_DEPLOYMENT \
                     (the deployment name, not the model name)"
                        .into(),
                )
            })?;
        let api_key = astria_core::env_var("AZURE_API_KEY")
            .or_else(|| std::env::var("AZURE_OPENAI_API_KEY").ok())
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| {
                AstriaError::Graph("no Azure API key: set ASTRIA_AZURE_API_KEY".into())
            })?;
        let api_version = astria_core::env_var("AZURE_API_VERSION")
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "2024-10-21".into());
        let model = astria_core::env_var("LLM_MODEL")
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| "gpt-4o-mini".into());

        let inner = OpenAiBackend::new_with_auth(
            Some(api_key),
            format!("{endpoint}/openai/deployments/{deployment}"),
            model,
            crate::backend_openai::AuthStyle::ApiKeyHeader,
        )
        .with_query_suffix(format!("?api-version={}", api_version.trim()));
        Ok(Self {
            inner,
            endpoint,
            deployment,
            api_version: api_version.trim().to_string(),
        })
    }

    /// Test/DI constructor: no env access.
    pub fn from_parts(
        endpoint: String,
        deployment: String,
        api_key: String,
        api_version: String,
        model: String,
    ) -> Self {
        let inner = OpenAiBackend::new_with_auth(
            Some(api_key),
            format!("{endpoint}/openai/deployments/{deployment}"),
            model,
            crate::backend_openai::AuthStyle::ApiKeyHeader,
        )
        .with_query_suffix(format!("?api-version={api_version}"));
        Self {
            inner,
            endpoint,
            deployment,
            api_version,
        }
    }

    /// The chat-completions URL a request is POSTed to (test-visible; the
    /// API key never appears in it).
    pub fn url(&self) -> String {
        format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            self.endpoint, self.deployment, self.api_version
        )
    }
}

impl SemanticBackend for AzureOpenAiBackend {
    fn cache_identity(&self) -> String {
        format!(
            "azure:{}:{}:{}",
            self.endpoint, self.deployment, self.api_version
        )
    }

    fn extract_semantic(&self, content: &str, file_type: &str) -> Result<SemanticExtraction> {
        self.inner.extract_semantic(content, file_type)
    }

    fn extract_semantic_from_image(
        &self,
        image_bytes: &[u8],
        media_type: &str,
    ) -> Result<SemanticExtraction> {
        self.inner
            .extract_semantic_from_image(image_bytes, media_type)
    }

    fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.inner.complete(system, user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ENV_LOCK;

    #[test]
    fn azure_resolution_requires_all_three_settings() {
        let _guard = ENV_LOCK.lock().unwrap();
        for (var, value) in [
            ("ASTRIA_AZURE_ENDPOINT", "https://res.openai.azure.com"),
            ("ASTRIA_AZURE_DEPLOYMENT", "gpt4o"),
            ("ASTRIA_AZURE_API_KEY", "key"),
        ] {
            std::env::remove_var("ASTRIA_AZURE_ENDPOINT");
            std::env::remove_var("ASTRIA_AZURE_DEPLOYMENT");
            std::env::remove_var("ASTRIA_AZURE_API_KEY");
            std::env::set_var(var, value);
            let err = AzureOpenAiBackend::from_env()
                .err()
                .expect("partial Azure config must error");
            std::env::remove_var(var);
            let _ = err;
        }
        std::env::remove_var("ASTRIA_AZURE_ENDPOINT");
        std::env::remove_var("ASTRIA_AZURE_DEPLOYMENT");
        std::env::remove_var("ASTRIA_AZURE_API_KEY");
    }

    #[test]
    fn azure_url_routes_through_deployment_with_api_version() {
        let backend = AzureOpenAiBackend::from_parts(
            "https://res.openai.azure.com".into(),
            "gpt4o-deploy".into(),
            "key".into(),
            "2024-10-21".into(),
            "gpt-4o-mini".into(),
        );
        assert_eq!(
            backend.url(),
            "https://res.openai.azure.com/openai/deployments/gpt4o-deploy/chat/completions?api-version=2024-10-21"
        );
        assert_eq!(
            backend.cache_identity(),
            "azure:https://res.openai.azure.com:gpt4o-deploy:2024-10-21"
        );
    }

    #[test]
    fn azure_request_body_is_openai_compatible() {
        let backend = AzureOpenAiBackend::from_parts(
            "https://res.openai.azure.com".into(),
            "d".into(),
            "k".into(),
            "2024-10-21".into(),
            "gpt-4o-mini".into(),
        );
        let body = backend.inner.build_request_body("hello", "rust");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "hello");
    }
}
