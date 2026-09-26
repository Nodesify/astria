//! Configuration management for the graph processing pipeline.
//!
//! Provides types and helpers for loading, validating, and merging
//! pipeline configuration from TOML files and environment overrides.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Supported output formats for the graph report.
#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Markdown,
    Json,
    Dot,
}

/// Configuration for a single extraction pass.
#[derive(Debug, Clone)]
pub struct ExtractConfig {
    /// File extensions to include (e.g. `["py", "rs"]`).
    pub extensions: Vec<String>,
    /// Maximum file size in bytes; larger files are skipped.
    pub max_file_size: usize,
    /// Language-specific overrides keyed by extension.
    pub lang_overrides: HashMap<String, HashMap<String, String>>,
}

impl Default for ExtractConfig {
    fn default() -> Self {
        Self {
            extensions: vec![
                "py".into(), "js".into(), "ts".into(), "rs".into(),
                "go".into(), "java".into(), "c".into(), "cpp".into(),
            ],
            max_file_size: 1_000_000,
            lang_overrides: HashMap::new(),
        }
    }
}

/// Top-level pipeline configuration.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Root directory to scan.
    pub root: PathBuf,
    /// Output format for the final report.
    pub output_format: OutputFormat,
    /// Extraction sub-config.
    pub extract: ExtractConfig,
    /// Number of worker threads for parallel extraction.
    pub workers: usize,
}

impl PipelineConfig {
    /// Create a new config with the given root directory and sensible defaults.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            output_format: OutputFormat::Markdown,
            extract: ExtractConfig::default(),
            workers: 4,
        }
    }

    /// Load configuration from a TOML file, falling back to defaults.
    pub fn load_from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        // WHY: We parse manually instead of pulling in `serde` to keep
        // the dependency tree small for this utility crate.
        let mut config = PipelineConfig::new(path.parent().unwrap_or(Path::new(".")));
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "workers" => {
                        config.workers = value.trim().parse()?;
                    }
                    "max_file_size" => {
                        config.extract.max_file_size = value.trim().parse()?;
                    }
                    _ => {}
                }
            }
        }
        Ok(config)
    }

    /// Merge environment variable overrides into this configuration.
    pub fn apply_env_overrides(&mut self) {
        // NOTE: Env vars take precedence over file values so that CI
        // pipelines can override without modifying config files.
        if let Ok(val) = std::env::var("GRAPHIFY_WORKERS") {
            if let Ok(n) = val.parse::<usize>() {
                self.workers = n;
            }
        }
        if let Ok(val) = std::env::var("GRAPHIFY_MAX_FILE_SIZE") {
            if let Ok(n) = val.parse::<usize>() {
                self.extract.max_file_size = n;
            }
        }
    }
}

/// Validate that a config points to an accessible directory.
pub fn validate_config(config: &PipelineConfig) -> Result<(), String> {
    if !config.root.exists() {
        return Err(format!("Root path does not exist: {}", config.root.display()));
    }
    if !config.root.is_dir() {
        return Err(format!("Root path is not a directory: {}", config.root.display()));
    }
    if config.workers == 0 {
        return Err("workers must be at least 1".into());
    }
    Ok(())
}

/// Convenience: create a validated config from a directory path.
pub fn init_config(path: &str) -> Result<PipelineConfig, Box<dyn std::error::Error>> {
    let mut config = PipelineConfig::new(path);
    config.apply_env_overrides();
    validate_config(&config)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_extensions() {
        let config = PipelineConfig::new("/tmp");
        assert!(!config.extract.extensions.is_empty());
    }
}
