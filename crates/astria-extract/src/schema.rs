use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractedNode {
    pub id: String,
    pub label: String,
    pub source_file: PathBuf,
    pub source_line: Option<u32>,
    pub docstring: Option<String>,
    pub node_type: String,
    /// Source text of the item up to its body (e.g. a function signature),
    /// whitespace-collapsed. Lets agents see WHAT a symbol is without
    /// opening the file. Absent for nodes without a meaningful signature
    /// (documents, stubs).
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractedEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub confidence: String,
    pub confidence_score: Option<f64>,
    pub source_file: PathBuf,
    pub source_line: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Extraction {
    pub file_path: PathBuf,
    pub language: String,
    pub nodes: Vec<ExtractedNode>,
    pub edges: Vec<ExtractedEdge>,
    /// How completely this file was understood.
    pub status: ExtractionStatus,
    /// Parser-error locations ("line N") and other status detail, capped to
    /// a few entries per file.
    pub notes: Vec<String>,
}

/// Per-file extraction outcome, so consumers can distinguish "no
/// relationship exists" from "this file was not fully understood".
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionStatus {
    /// Parsed fully; the file's symbols and relationships are represented
    /// (a data-only file with no symbols is still complete).
    Complete,
    /// The parser recovered from errors — some constructs may be missing.
    /// `notes` carries the first error locations.
    Partial,
    /// No extractor exists for this file: unknown suffix, or the language's
    /// grammar is not compiled into this build.
    Unsupported,
    /// Intentionally not extracted this run (missing transcription tooling,
    /// absent workspace credentials) — retryable, never cached.
    Skipped,
    /// A code file that parsed without errors yet yielded no symbols —
    /// unexpected symbol loss, not a data-only file.
    SuspiciousEmpty,
}

impl ExtractionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ExtractionStatus::Complete => "complete",
            ExtractionStatus::Partial => "partial",
            ExtractionStatus::Unsupported => "unsupported",
            ExtractionStatus::Skipped => "skipped",
            ExtractionStatus::SuspiciousEmpty => "suspicious_empty",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "partial" => ExtractionStatus::Partial,
            "unsupported" => ExtractionStatus::Unsupported,
            "skipped" => ExtractionStatus::Skipped,
            "suspicious_empty" => ExtractionStatus::SuspiciousEmpty,
            _ => ExtractionStatus::Complete,
        }
    }
}

impl Extraction {
    /// A fully-extracted file (the common case) with no caveats.
    pub fn complete(
        file_path: PathBuf,
        language: impl Into<String>,
        nodes: Vec<ExtractedNode>,
        edges: Vec<ExtractedEdge>,
    ) -> Self {
        Extraction {
            file_path,
            language: language.into(),
            nodes,
            edges,
            status: ExtractionStatus::Complete,
            notes: Vec::new(),
        }
    }

    /// An extraction whose outcome is the status itself — nothing was
    /// extracted (unsupported suffixes, skipped media, missing tooling).
    pub fn status_only(
        file_path: PathBuf,
        language: impl Into<String>,
        status: ExtractionStatus,
        note: &str,
    ) -> Self {
        Extraction {
            file_path,
            language: language.into(),
            nodes: Vec::new(),
            edges: Vec::new(),
            status,
            notes: if note.is_empty() {
                Vec::new()
            } else {
                vec![note.to_string()]
            },
        }
    }
}
