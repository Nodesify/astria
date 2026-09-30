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
pub const EXTRACTION_HASH_VERSION: &str = "v11";

/// Reads `ASTRIA_<name>`, falling back to the deprecated `GRAPHIFY_<name>`
/// spelling so pre-1.0 env configs keep working. The new name wins; an empty
/// `ASTRIA_` value falls through to the legacy name.
pub fn env_var(name: &str) -> Option<String> {
    std::env::var(format!("ASTRIA_{name}"))
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var(format!("GRAPHIFY_{name}")).ok())
}

pub use db::{open_db, open_db_in_memory};
pub use error::{AstriaError, Result};
pub use security::{check_file_size, sanitize_docstring, sanitize_label, validate_path};
pub use types::*;
