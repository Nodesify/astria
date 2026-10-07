//! Persist only the effective non-secret indexing policy.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct IndexingProfile {
    pub version: u32,
    pub semantic_configuration: String,
    pub environment: BTreeMap<String, String>,
    pub dedup: bool,
    pub embed: bool,
    pub label_communities: bool,
    pub deep: bool,
}
const SETTINGS: &[&str] = &[
    "LLM_BACKEND",
    "LLM_MODEL",
    "LLM_BASE_URL",
    "LLM_JUDGE",
    "KIMI_BASE_URL",
    "AZURE_ENDPOINT",
    "AZURE_DEPLOYMENT",
    "AZURE_API_VERSION",
    "AWS_REGION",
    "LLM_BUDGET",
];
fn setting(key: &str) -> Option<String> {
    astria_core::env_var(key).or_else(|| match key {
        "LLM_BASE_URL" => std::env::var("OPENAI_BASE_URL").ok(),
        "AZURE_ENDPOINT" => std::env::var("AZURE_OPENAI_ENDPOINT").ok(),
        "AZURE_DEPLOYMENT" => std::env::var("AZURE_OPENAI_DEPLOYMENT_NAME").ok(),
        "AWS_REGION" => std::env::var("AWS_REGION")
            .ok()
            .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok()),
        _ => None,
    })
}
pub(super) fn load(root: &Path) -> astria_core::Result<Option<IndexingProfile>> {
    match std::fs::read(root.join(".astria/indexing-profile.json")) {
        Ok(bytes) => {
            let profile: IndexingProfile = serde_json::from_slice(&bytes)?;
            if profile.version != 1 {
                return Err(astria_core::AstriaError::Graph(
                    "unsupported indexing profile version".into(),
                ));
            }
            Ok(Some(profile))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
impl IndexingProfile {
    pub(super) fn capture(options: &PipelineOptions) -> astria_core::Result<Self> {
        let backend = setting("LLM_BACKEND")
            .unwrap_or_else(|| "none".into())
            .trim()
            .to_lowercase();
        let mut environment = BTreeMap::new();
        environment.insert("ASTRIA_LLM_BACKEND".into(), backend.clone());
        let mut capture = |key: &str, default: Option<&str>| -> astria_core::Result<()> {
            if let Some(value) = setting(key).or_else(|| default.map(str::to_owned)) {
                if key.ends_with("URL") || key == "AZURE_ENDPOINT" {
                    let url = url::Url::parse(&value).map_err(|_| {
                        astria_core::AstriaError::Graph(format!("invalid endpoint {key}"))
                    })?;
                    if !url.username().is_empty()
                        || url.password().is_some()
                        || url.query().is_some()
                        || url.fragment().is_some()
                    {
                        return Err(astria_core::AstriaError::Graph(format!("{key} must not contain credentials, query parameters, or fragments; use credential environment variables")));
                    }
                }
                environment.insert(format!("ASTRIA_{key}"), value);
            }
            Ok(())
        };
        capture("LLM_BUDGET", None)?;
        if backend != "none" {
            capture("LLM_JUDGE", None)?;
        }
        match backend.as_str() {
            "claude" => capture("LLM_MODEL", Some("claude-sonnet-4-20250514"))?,
            "openai" => {
                capture("LLM_MODEL", Some("gpt-4o-mini"))?;
                capture("LLM_BASE_URL", Some("https://api.openai.com/v1"))?;
            }
            "azure" => {
                capture("LLM_MODEL", Some("gpt-4o-mini"))?;
                capture("AZURE_ENDPOINT", None)?;
                capture("AZURE_DEPLOYMENT", None)?;
                capture("AZURE_API_VERSION", Some("2024-10-21"))?;
            }
            "bedrock" => {
                capture("LLM_MODEL", None)?;
                capture("AWS_REGION", None)?;
            }
            "kimi" => {
                capture("LLM_MODEL", Some("kimi-k2-0905-preview"))?;
                capture("KIMI_BASE_URL", Some("https://api.moonshot.cn/v1"))?;
            }
            "gemini" => capture("LLM_MODEL", Some("gemini-2.0-flash"))?,
            "none" => {}
            _ => {
                return Err(astria_core::AstriaError::Graph(format!(
                    "unsupported indexing backend {backend}"
                )))
            }
        }
        Ok(Self {
            version: 1,
            semantic_configuration: super::semantic_pass::configuration()?,
            environment,
            dedup: options.dedup,
            embed: options.embed,
            label_communities: options.label_communities,
            deep: options.deep,
        })
    }
    pub(super) fn validate_change(&self, root: &Path) -> astria_core::Result<()> {
        if let Some(previous) = load(root)? {
            let paid = |p: &Self| {
                p.environment
                    .get("ASTRIA_LLM_BACKEND")
                    .is_some_and(|b| b != "none")
            };
            let mut old = previous.environment.clone();
            let mut current = self.environment.clone();
            old.remove("ASTRIA_LLM_BUDGET");
            current.remove("ASTRIA_LLM_BUDGET");
            let changed = previous.semantic_configuration != self.semantic_configuration
                || old != current
                || previous.deep != self.deep
                || previous.label_communities != self.label_communities;
            if changed
                && (paid(&previous) || paid(self))
                && (astria_core::env_var("REFRESH_POLICY").as_deref() != Some("1")
                    || (paid(self) && astria_semantic::enrichment::budget_from_env() == 0))
            {
                return Err(astria_core::AstriaError::Graph("changing paid indexing policy requires --refresh-policy and --llm-budget <positive tokens>".into()));
            }
        }
        Ok(())
    }
    pub(super) fn apply(&self) {
        for key in SETTINGS {
            std::env::remove_var(format!("ASTRIA_{key}"));
        }
        for (key, value) in &self.environment {
            // Profiles never get to inject arbitrary environment variables.
            if SETTINGS
                .iter()
                .any(|allowed| key == &format!("ASTRIA_{allowed}"))
            {
                std::env::set_var(key, value);
            }
        }
    }
    pub(super) fn save(&self, root: &Path) -> astria_core::Result<()> {
        astria_core::writer_lock::write_atomic(
            &root.join(".astria/indexing-profile.json"),
            &serde_json::to_vec_pretty(self)?,
        )
    }
}

/// Restore the caller's exact environment, including non-Unicode values.
struct ScopedEnvironment(Vec<(String, Option<std::ffi::OsString>)>);
impl ScopedEnvironment {
    fn new() -> Self {
        let mut names: Vec<String> = SETTINGS.iter().map(|key| format!("ASTRIA_{key}")).collect();
        names.push("ASTRIA_REFRESH_POLICY".into());
        Self(
            names
                .into_iter()
                .map(|key| {
                    let previous = std::env::var_os(&key);
                    (key, previous)
                })
                .collect(),
        )
    }
}
impl Drop for ScopedEnvironment {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

/// Ingestion inherits the published project's policy while holding its guard.
pub fn run_pipeline_using_profile_locked(
    root: &Path,
    cli_version: Option<&str>,
    guard: &astria_core::writer_lock::WriterLock,
) -> astria_core::Result<PipelineResult> {
    // All callers acquire the project lock before this process-wide lock.
    let _state = super::lock_pipeline_state();
    let _environment = ScopedEnvironment::new();
    std::env::remove_var("ASTRIA_REFRESH_POLICY");
    if let Some(profile) = load(root)? {
        profile.apply();
        super::run_pipeline_under_state_lock(
            root,
            profile.dedup,
            profile.embed,
            profile.label_communities,
            profile.deep,
            cli_version,
            guard,
        )
    } else {
        // An automatic update must never discover a paid backend from shell defaults.
        for key in SETTINGS {
            std::env::remove_var(format!("ASTRIA_{key}"));
        }
        std::env::set_var("ASTRIA_LLM_BACKEND", "none");
        super::run_pipeline_under_state_lock(root, true, false, false, false, cli_version, guard)
    }
}
