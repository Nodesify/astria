pub mod builtins;
pub mod cache;
pub mod docs;
pub mod engine;
pub mod langs;
pub mod manifest;
pub mod naming;
pub mod refs;
pub mod schema;
pub mod walkers;

pub use engine::extract;
pub use refs::resolve_cross_file_references;
pub use schema::{ExtractedEdge, ExtractedNode, Extraction};
pub use walkers::{declaration_spans, extract_source};

/// The content-hash family the extraction layer stores under: the plain
/// file hash, or that hash suffixed with `:gws-rev:<revision>` for Google
/// Workspace shortcuts (whose local bytes never change). Consumers that
/// must decide whether a stored row (e.g. `derived_text`) is fresh compare
/// their recomputed plain hash against either spelling.
pub fn is_extraction_hash_for(stored_hash: &str, plain_hash: &str) -> bool {
    stored_hash == plain_hash
        || stored_hash
            .strip_prefix(plain_hash)
            .is_some_and(|rest| rest.starts_with(":gws-rev:"))
}
