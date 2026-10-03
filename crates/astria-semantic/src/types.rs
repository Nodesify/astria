//! Shared extraction types and the `SemanticBackend` trait every
//! backend implements.
#![allow(unused_imports)]

use super::*;
use astria_core::AstriaError;
use astria_core::Result;
use base64::Engine as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

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
    /// Calibrated existence confidence (0..=1) from the Jev verification
    /// pass. Absent for engine-only extractions; persisted through the
    /// merge path into `edges.confidence_score`.
    #[serde(default)]
    pub confidence_score: Option<f64>,
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
    /// Non-secret effective configuration used to invalidate cached output.
    fn cache_identity(&self) -> String {
        std::any::type_name::<Self>().to_string()
    }

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

    /// Single-shot completion for the auxiliary passes (community naming,
    /// deep concept linking). One request, no chunking; the model's raw
    /// text comes back for the caller to parse. Every response is counted
    /// in the usage tracker.
    fn complete(&self, _system: &str, _user: &str) -> Result<String> {
        Err(AstriaError::Graph(
            "this backend does not support auxiliary completions".into(),
        ))
    }

    /// Optional batch gate: return the subset of `files` worth enriching.
    /// The default keeps everything; decision backends (Jev) may drop files
    /// they judge trivial so no engine call is ever spent on them.
    fn gate_files(&self, files: &[PathBuf]) -> Vec<PathBuf> {
        files.to_vec()
    }

    /// Optional suggested-question ranking: a permutation of
    /// `0..questions.len()` in preferred (most useful first) order. The
    /// default keeps the generated order.
    fn rank_questions(&self, questions: &[String]) -> Vec<usize> {
        (0..questions.len()).collect()
    }
}

// ---------------------------------------------------------------------------
// Shared prompt + parsing
// ---------------------------------------------------------------------------
