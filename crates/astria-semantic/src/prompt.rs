//! Prompt construction and response discipline: the extraction system
//! and vision prompts, reply parsing, schema clamping, and output
//! sanitization.
#![allow(unused_imports)]

use super::*;
use astria_core::AstriaError;
use astria_core::Result;
use base64::Engine as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub(crate) fn system_prompt(file_type: &str) -> String {
    format!(
        "You are a knowledge graph extraction engine. Given the following {file_type} content, \
         extract semantic topics, concepts, and entities as nodes, and the relationships \
         between them as edges. Respond ONLY with valid JSON in this exact format:\n\
         {{\"nodes\": [{{\"id\": \"...\", \"label\": \"...\", \"summary\": \"...\", \"node_type\": \"...\"}}], \
         \"edges\": [{{\"source\": \"node_id\", \"target\": \"node_id\", \"relation\": \"...\"}}]}}\n\
         Use concise lowercase IDs (e.g. \"error_handling\"). \
         node_type should be one of: concept, entity, pattern, module, function.\n\
         relation should be one of: depends_on, implements, relates_to, contains, uses.\n\
         Return an empty JSON object if the content is too short or uninformative."
    )
}

pub(crate) fn vision_prompt() -> String {
    "You are a knowledge graph extraction engine. The user message contains an image \
     (a screenshot, diagram, whiteboard photo, chart, or slide). Extract the visible \
     concepts, entities, and their relationships as a knowledge graph. Respond ONLY \
     with valid JSON in the same format used for text extraction: \
     {\"nodes\": [...], \"edges\": [...]}. If the image has no meaningful content, \
     return an empty JSON object."
        .to_string()
}

/// Parse the model's text reply into an extraction, tolerating surrounding
/// prose by falling back to the outermost {...} span. The result is always
/// sanitized (see `sanitize_extraction`).
pub(crate) fn parse_extraction_text(text: &str) -> SemanticExtraction {
    if text.trim().is_empty() {
        return SemanticExtraction::empty();
    }
    if let Ok(parsed) = serde_json::from_str::<SemanticExtraction>(text.trim()) {
        return sanitize_extraction(parsed);
    }
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        if let Ok(parsed) = serde_json::from_str::<SemanticExtraction>(&text[start..=end]) {
            return sanitize_extraction(parsed);
        }
    }
    SemanticExtraction::empty()
}

/// node_type values the schema allows; anything else is clamped.
pub(crate) const ALLOWED_NODE_TYPES: &[&str] =
    &["concept", "entity", "pattern", "module", "function"];
/// relation values the schema allows; anything else is clamped.
pub(crate) const ALLOWED_RELATIONS: &[&str] =
    &["depends_on", "implements", "relates_to", "contains", "uses"];

/// Enforce output discipline on model responses: drop empty/duplicate
/// nodes, clamp node_type/relation to the schema enums, and drop edges
/// whose endpoints were not returned as nodes (they would become dangling
/// stubs in the graph).
pub(crate) fn sanitize_extraction(mut ext: SemanticExtraction) -> SemanticExtraction {
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    ext.nodes.retain(|node| {
        !node.id.trim().is_empty()
            && !node.label.trim().is_empty()
            && seen_ids.insert(node.id.trim().to_string())
    });
    for node in &mut ext.nodes {
        node.id = node.id.trim().to_string();
        if !ALLOWED_NODE_TYPES.contains(&node.node_type.as_str()) {
            node.node_type = "concept".to_string();
        }
    }
    let valid_ids: std::collections::HashSet<&str> =
        ext.nodes.iter().map(|n| n.id.as_str()).collect();

    let mut seen_edges: std::collections::HashSet<(String, String, String)> =
        std::collections::HashSet::new();
    ext.edges.retain(|edge| {
        edge.source != edge.target
            && valid_ids.contains(edge.source.as_str())
            && valid_ids.contains(edge.target.as_str())
            && seen_edges.insert((
                edge.source.clone(),
                edge.target.clone(),
                edge.relation.clone(),
            ))
    });
    for edge in &mut ext.edges {
        if !ALLOWED_RELATIONS.contains(&edge.relation.as_str()) {
            edge.relation = "relates_to".to_string();
        }
    }
    ext
}
