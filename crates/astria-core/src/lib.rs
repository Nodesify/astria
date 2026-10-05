// astria-core: core types, database schema, and pipeline orchestration

pub mod calibration;
pub mod db;
pub mod error;
pub mod ids;
pub mod languages;
pub mod security;
pub mod types;

/// Version tag mixed into every content hash (detect manifest + extraction
/// cache). Bump when extraction output changes shape (e.g. the id scheme) —
/// all files then hash differently, forcing one clean full re-extraction on
/// upgrade instead of mixing old and new node ids in one graph.
/// v4: impl methods scope under their impl type; repeated md headings get
/// unique ids; empty-endpoint edges are dropped.
/// v5: impl blocks are scope-only containers (no duplicate type node).
/// v6: cfg-gated twin definitions dedup to the first occurrence.
/// v7: complete-corpus reference reconciliation and strict extraction errors.
/// v8: preserve scoped definitions, assigned functions, test roles and Python implementations.
/// v9: chunk document body text into searchable section chunks (markdown, text, RST).
/// v10: chunk overlap across boundaries, `chunk` node type, ASTRIA_CHUNK_CHARS override.
/// v11: Rust `pub`/documented consts and statics extracted as `constant` nodes
/// (label + doc comment + initializer signature).
/// v12: Rust `///` item doc comments and `//!` module docs captured as
/// docstrings; uniquely-resolved call edges carry `RESOLVED` provenance.
/// v13: video/audio files route through whisper transcription instead of
/// empty `media` extractions; invalidates the empty cached results.
/// v14: case-preserving structural node ids (path-digest file stems,
/// declaration disambiguators, disambiguation-aware edge rewiring); all
/// previously cached extractions carry the old folded ids and must be
/// re-extracted.
/// v15: qualified call boundaries and exact import module paths retained.
pub const EXTRACTION_HASH_VERSION: &str = "v15";

/// Reads `ASTRIA_<name>`. Empty values count as unset. There is no legacy
/// spelling fallback: pre-1.0 configurations rebuild with current names.
pub fn env_var(name: &str) -> Option<String> {
    std::env::var(format!("ASTRIA_{name}"))
        .ok()
        .filter(|value| !value.is_empty())
}

pub use db::{open_db, open_db_in_memory};
pub use error::{AstriaError, Result};
pub use security::{check_file_size, sanitize_docstring, sanitize_label, validate_path};
pub use types::*;
