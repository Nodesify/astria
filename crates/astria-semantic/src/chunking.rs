//! Long-content handling: chunk splitting, per-chunk merge, and the
//! bounded chunked-extraction driver.
#![allow(unused_imports)]

use super::*;
use astria_core::AstriaError;
use astria_core::Result;
use base64::Engine as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Split `content` into chunks of at most MAX_CHUNK_CHARS characters,
/// preferring line boundaries so extractions see coherent code blocks.
pub(crate) fn split_chunks(content: &str) -> Vec<String> {
    if content.chars().count() <= MAX_CHUNK_CHARS {
        return vec![content.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for line in content.split_inclusive('\n') {
        let line_len = line.chars().count();
        if current_len > 0 && current_len + line_len > MAX_CHUNK_CHARS {
            chunks.push(std::mem::take(&mut current));
            current_len = 0;
        }
        current_len += line_len;
        current.push_str(line);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Merge per-chunk extractions: nodes deduplicated by id (first wins),
/// edges concatenated. Sanitization happens afterwards against the merged
/// node set so cross-chunk references survive.
pub(crate) fn merge_extractions(parts: Vec<SemanticExtraction>) -> SemanticExtraction {
    let mut merged = SemanticExtraction::empty();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for part in parts {
        for node in part.nodes {
            if seen.insert(node.id.clone()) {
                merged.nodes.push(node);
            }
        }
        merged.edges.extend(part.edges);
    }
    merged
}

/// Extract from possibly-long content: chunk it, extract each chunk, merge,
/// then sanitize the combined result. Bounded to MAX_CHUNKS API calls.
pub(crate) fn extract_content_chunked<F>(
    content: &str,
    file_type: &str,
    raw: F,
) -> Result<SemanticExtraction>
where
    F: Fn(&str, &str) -> Result<SemanticExtraction>,
{
    let chunks = split_chunks(content);
    let mut parts = Vec::new();
    for chunk in chunks.iter().take(MAX_CHUNKS) {
        parts.push(raw(chunk, file_type)?);
    }
    Ok(sanitize_extraction(merge_extractions(parts)))
}
