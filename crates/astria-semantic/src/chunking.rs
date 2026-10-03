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
/// preferring line boundaries so extractions see coherent code blocks. A
/// single line longer than the cap (minified bundles, base64 blobs) is
/// hard-split at char boundaries — never silently skipped or sent oversized.
pub(crate) fn split_chunks(content: &str) -> Vec<String> {
    if content.chars().count() <= MAX_CHUNK_CHARS {
        return vec![content.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for line in content.split_inclusive('\n') {
        let line_len = line.chars().count();
        if line_len > MAX_CHUNK_CHARS {
            // Flush what's accumulated, then hard-split the giant line.
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
                current_len = 0;
            }
            let chars: Vec<char> = line.chars().collect();
            for piece in chars.chunks(MAX_CHUNK_CHARS) {
                chunks.push(piece.iter().collect());
            }
            continue;
        }
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
/// then sanitize the combined result. Content beyond the chunk cap is a
/// HARD ERROR, not a partial success: a truncated extraction must never be
/// cached as if it described the whole file. Raise the cap with
/// `ASTRIA_LLM_MAX_CHUNKS` (1..=64) or split the file. Every chunk claims
/// its own budget reservation — chunk calls are the billable requests.
pub(crate) fn extract_content_chunked<F>(
    content: &str,
    file_type: &str,
    raw: F,
) -> Result<SemanticExtraction>
where
    F: Fn(&str, &str) -> Result<SemanticExtraction>,
{
    let cap = crate::max_chunks();
    let chunks = split_chunks(content);
    if chunks.len() > cap {
        let skipped: usize = chunks[cap..].iter().map(|c| c.chars().count()).sum();
        return Err(AstriaError::Graph(format!(
            "{file_type} content needs {} chunks, over the {cap}-chunk extraction cap; \
             ~{skipped} characters would have gone unenriched and a truncated result must not \
             be published as complete — raise ASTRIA_LLM_MAX_CHUNKS or split the file",
            chunks.len()
        )));
    }
    let mut parts = Vec::new();
    for chunk in &chunks {
        // Per-request reservation: input estimate for this chunk plus the
        // full output allowance the wire request permits.
        let _reservation = crate::enrichment::reserve_budget(
            chunk.chars().count(),
            crate::enrichment::MAX_OUTPUT_TOKENS_EXTRACT,
        )?;
        parts.push(raw(chunk, file_type)?);
    }
    Ok(sanitize_extraction(merge_extractions(parts)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_lines_are_split_at_char_boundaries() {
        let giant = "x".repeat(MAX_CHUNK_CHARS * 2 + 10);
        let content = format!("short line\n{giant}\ntail\n");
        let chunks = split_chunks(&content);
        assert!(chunks.iter().all(|c| c.chars().count() <= MAX_CHUNK_CHARS));
        // Nothing lost: concatenation preserves every character.
        assert_eq!(chunks.concat(), content);
    }

    #[test]
    fn normal_content_splits_on_line_boundaries() {
        let line = "a".repeat(MAX_CHUNK_CHARS / 2) + "\n";
        let content = line.repeat(5);
        let chunks = split_chunks(&content);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|c| c.chars().count() <= MAX_CHUNK_CHARS));
        assert_eq!(chunks.concat(), content);
    }

    #[test]
    fn chunk_cap_is_a_hard_error_not_partial_success() {
        // extract_content_chunked reserves from the process-global budget;
        // hold the shared lock so a parallel budget test cannot cap it.
        let _guard = crate::BUDGET_TEST_LOCK.lock().unwrap();
        crate::enrichment::configure_budget(0);
        // Content needing more than the chunk cap must FAIL: a truncated
        // extraction returned as success would be cached as if it described
        // the whole file.
        let chunk = "b".repeat(MAX_CHUNK_CHARS) + "\n";
        let content = chunk.repeat(max_chunks() + 2);
        let err = extract_content_chunked(&content, "text", |_, _| Ok(SemanticExtraction::empty()))
            .expect_err("oversized content must error");
        assert!(err.to_string().contains("ASTRIA_LLM_MAX_CHUNKS"), "{err}");
    }

    #[test]
    fn within_cap_content_extracts_every_chunk() {
        let _guard = crate::BUDGET_TEST_LOCK.lock().unwrap();
        crate::enrichment::configure_budget(0);
        // Two chunks of exactly the cap (no trailing newline, which would
        // push each over and hard-split them into four pieces).
        let chunk = "c".repeat(MAX_CHUNK_CHARS);
        let content = format!("{chunk}{chunk}");
        let seen = std::cell::RefCell::new(0usize);
        let result = extract_content_chunked(&content, "text", |_, _| {
            *seen.borrow_mut() += 1;
            Ok(SemanticExtraction::empty())
        })
        .unwrap();
        assert_eq!(result.nodes.len(), 0);
        assert_eq!(*seen.borrow(), 2);
    }
}
