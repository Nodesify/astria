//! The Jev judge layer wrapped around a selected engine backend:
//! re-judged node types and relations, calibrated edge confidence,
//! and the batch trivial-file gate.
#![allow(unused_imports)]

use super::*;
use astria_core::AstriaError;
use astria_core::Result;
use base64::Engine as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// A judge layer that wraps the selected engine backend (Claude /
/// OpenAI-compatible / Gemini). Jev cannot generate the node/edge JSON
/// itself — it returns typed judgments with calibrated probabilities — so
/// this backend first runs the engine, then re-judges the extraction: node
/// types and relations are re-chosen from the schema allowlists, every edge
/// gets a keep/drop existence verdict, and the winning probability becomes
/// the edge's `confidence_score`. The engine also handles the auxiliary
/// `complete()` passes unchanged, and the Jev batch gate may skip trivial
/// files before their first extraction.
///
/// Selected with `--judge jev` / `ASTRIA_LLM_JUDGE=jev` on top of an
/// explicit `--backend`; it is never a backend itself.
pub struct JevJudgeBackend {
    engine: Box<dyn SemanticBackend>,
    client: jev::JevClient,
    config: jev::JevConfig,
}

impl JevJudgeBackend {
    /// Wrap an already-resolved engine backend. Judge configuration comes
    /// from the environment:
    ///
    /// - `ASTRIA_LLM_JUDGE_API_KEY` (or `TYPESAFE_API_KEY`) — required.
    /// - `ASTRIA_LLM_JUDGE_MODEL` — optional, defaults to `jev-latest`.
    /// - `ASTRIA_LLM_JEV_VERIFY` / `ASTRIA_LLM_JEV_MIN_EDGE_PROBABILITY` —
    ///   verification pass controls (see `jev::JevConfig`).
    /// - `ASTRIA_LLM_JEV_GATE` / `ASTRIA_LLM_JEV_GATE_*` — gate controls.
    pub fn new(engine: Box<dyn SemanticBackend>, config: jev::JevConfig) -> Self {
        Self {
            client: jev::JevClient::new(config.clone()),
            engine,
            config,
        }
    }
}

impl SemanticBackend for JevJudgeBackend {
    fn cache_identity(&self) -> String {
        format!(
            "jev:{}:{}",
            self.engine.cache_identity(),
            self.config.identity()
        )
    }

    fn extract_semantic(&self, content: &str, file_type: &str) -> Result<SemanticExtraction> {
        let extraction = self.engine.extract_semantic(content, file_type)?;
        if !self.config.verify_enabled
            || (extraction.nodes.is_empty() && extraction.edges.is_empty())
        {
            return Ok(extraction);
        }
        // Edge existence is the only judgment that needs the file text;
        // an edgeless extraction re-chooses node types from labels and
        // summaries alone, so the (potentially large) content never ships.
        let content_ref = if extraction.edges.is_empty() {
            None
        } else {
            Some(content)
        };
        match self.client.verify(&extraction, content_ref, file_type) {
            Ok(verified) => Ok(verified),
            Err(e) => {
                // The engine already produced an extraction; a failed
                // verification must not lose it.
                eprintln!(
                    "warning: Jev verification unavailable ({e}); keeping the engine extraction unverified"
                );
                Ok(extraction)
            }
        }
    }

    fn extract_semantic_from_image(
        &self,
        image_bytes: &[u8],
        media_type: &str,
    ) -> Result<SemanticExtraction> {
        let extraction = self
            .engine
            .extract_semantic_from_image(image_bytes, media_type)?;
        if !self.config.verify_enabled
            || (extraction.nodes.is_empty() && extraction.edges.is_empty())
        {
            return Ok(extraction);
        }
        match self.client.verify(&extraction, None, media_type) {
            Ok(verified) => Ok(verified),
            Err(e) => {
                eprintln!(
                    "warning: Jev verification unavailable ({e}); keeping the engine extraction unverified"
                );
                Ok(extraction)
            }
        }
    }

    fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.engine.complete(system, user)
    }

    fn gate_files(&self, files: &[PathBuf]) -> Vec<PathBuf> {
        if !self.config.gate_enabled {
            return files.to_vec();
        }
        match self.client.gate_files(files, &self.config) {
            Ok(kept) => kept,
            Err(e) => {
                // The gate may only save calls, never lose facts.
                eprintln!("warning: Jev gate unavailable ({e}); enriching all candidate files");
                files.to_vec()
            }
        }
    }

    fn rank_questions(&self, questions: &[String]) -> Vec<usize> {
        match self.client.rank_questions(questions) {
            Ok(permutation) => permutation,
            Err(e) => {
                eprintln!(
                    "warning: Jev question ranking unavailable ({e}); keeping the generated order"
                );
                (0..questions.len()).collect()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Backend resolution
// ---------------------------------------------------------------------------
