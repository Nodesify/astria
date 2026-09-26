// docs: plain-text extractors for documentation formats (markdown, plain
// text, reStructuredText) — no tree-sitter involved.

use std::path::Path;

use crate::naming::{file_stem, make_node_id, make_target_id};
use crate::schema::{ExtractedEdge, ExtractedNode, Extraction};
use astria_core::AstriaError;

/// Extract markdown-style structure from a .md/.mdx file.
pub(crate) fn extract_markdown(path: &Path, naming: &Path) -> Result<Extraction, AstriaError> {
    let bytes = std::fs::read(path)?;
    let content = String::from_utf8_lossy(&bytes).into_owned();
    Ok(extract_markdown_from_string(
        path, "markdown", &content, naming,
    ))
}

/// Extract markdown-style structure from a string. Used for both .md/.mdx files
/// and PDF files (converted to markdown).
pub(crate) fn extract_markdown_from_string(
    path: &Path,
    language: &str,
    content: &str,
    naming: &Path,
) -> Extraction {
    let fid = file_stem(naming);
    let file_id = make_node_id(&[&fid]);

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Document node
    nodes.push(ExtractedNode {
        id: file_id.clone(),
        label: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string(),
        source_file: path.to_path_buf(),
        source_line: None,
        docstring: None,
        signature: None,
        node_type: "document".to_string(),
    });

    let heading_re = regex::Regex::new(r"^(#{1,6})\s+(.+)$").unwrap();
    let link_re = regex::Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap();

    // Track heading nesting: stack of (level, id)
    let mut heading_stack: Vec<(usize, String)> = Vec::new();
    // Repeated section titles in one file (e.g. several "## Changes") get
    // ordinal suffixes so ids stay unique within the extraction.
    let mut seen_section_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (line_no, line) in content.lines().enumerate() {
        // Parse headings
        if let Some(caps) = heading_re.captures(line) {
            let hashes = caps.get(1).unwrap().as_str().len();
            let title = caps.get(2).unwrap().as_str().trim().to_string();
            let level = hashes;
            let slug = make_target_id(&title);
            let section_id = {
                let base = make_node_id(&[&fid, &slug]);
                if seen_section_ids.insert(base.clone()) {
                    base
                } else {
                    let mut n = 2usize;
                    loop {
                        let candidate = make_node_id(&[&fid, &format!("{slug}-{n}")]);
                        if seen_section_ids.insert(candidate.clone()) {
                            break candidate;
                        }
                        n += 1;
                    }
                }
            };

            nodes.push(ExtractedNode {
                id: section_id.clone(),
                label: title,
                source_file: path.to_path_buf(),
                source_line: Some(line_no as u32 + 1),
                docstring: None,
                signature: None,
                node_type: "section".to_string(),
            });

            // Pop stack until we find a parent with lower level
            while let Some((parent_level, _)) = heading_stack.last() {
                if *parent_level < level {
                    break;
                }
                heading_stack.pop();
            }

            // Edge: parent heading → this heading, or file → this heading
            let parent_id = heading_stack
                .last()
                .map(|(_, id)| id.clone())
                .unwrap_or_else(|| file_id.clone());

            edges.push(ExtractedEdge {
                source: parent_id,
                target: section_id.clone(),
                relation: "contains".to_string(),
                confidence: "EXTRACTED".to_string(),
                confidence_score: Some(1.0),
                source_file: path.to_path_buf(),
                source_line: Some(line_no as u32 + 1),
            });

            heading_stack.push((level, section_id));
        }

        // Parse links (only local .md references)
        for cap in link_re.captures_iter(line) {
            let link_target = cap.get(2).unwrap().as_str();
            // Only reference local markdown files
            if link_target.starts_with("http") || link_target.starts_with('#') {
                continue;
            }
            let target_path = if link_target.starts_with('/') {
                link_target.to_string()
            } else {
                // Relative path — just use the file stem as target
                link_target.to_string()
            };
            let target_stem = std::path::Path::new(&target_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if !target_stem.is_empty() {
                let target_id = make_target_id(target_stem);
                edges.push(ExtractedEdge {
                    source: file_id.clone(),
                    target: target_id,
                    relation: "references".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    confidence_score: Some(0.8),
                    source_file: path.to_path_buf(),
                    source_line: Some(line_no as u32 + 1),
                });
            }
        }
    }

    Extraction {
        file_path: path.to_path_buf(),
        language: language.to_string(),
        nodes,
        edges,
    }
}

/// Extract structure from plain text files (.txt).
pub(crate) fn extract_text_file(
    path: &Path,
    language: &str,
    naming: &Path,
) -> Result<Extraction, AstriaError> {
    let bytes = std::fs::read(path)?;
    let content = String::from_utf8_lossy(&bytes).into_owned();
    let fid = file_stem(naming);
    let file_id = make_node_id(&[&fid]);

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    nodes.push(ExtractedNode {
        id: file_id.clone(),
        label: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string(),
        source_file: path.to_path_buf(),
        source_line: None,
        docstring: None,
        signature: None,
        node_type: "document".to_string(),
    });

    let mut line_no = 0u32;
    let mut para_index = 0u32;
    let mut current_para_start: Option<(u32, String)> = None;

    for line in content.lines() {
        line_no += 1;
        if line.trim().is_empty() {
            if let Some((start_line, text)) = current_para_start.take() {
                let label = truncate_with_ellipsis(&text, 80);
                let section_id = make_node_id(&[&fid, &format!("p{}", para_index)]);

                nodes.push(ExtractedNode {
                    id: section_id.clone(),
                    label,
                    source_file: path.to_path_buf(),
                    source_line: Some(start_line),
                    docstring: None,
                    signature: None,
                    node_type: "section".to_string(),
                });
                edges.push(ExtractedEdge {
                    source: file_id.clone(),
                    target: section_id,
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    confidence_score: Some(1.0),
                    source_file: path.to_path_buf(),
                    source_line: Some(start_line),
                });
                para_index += 1;
            }
        } else {
            if current_para_start.is_none() {
                current_para_start = Some((line_no, line.trim().to_string()));
            }
        }
    }
    // Handle trailing paragraph
    if let Some((start_line, text)) = current_para_start.take() {
        let label = truncate_with_ellipsis(&text, 80);
        let section_id = make_node_id(&[&fid, &format!("p{}", para_index)]);
        nodes.push(ExtractedNode {
            id: section_id.clone(),
            label,
            source_file: path.to_path_buf(),
            source_line: Some(start_line),
            docstring: None,
            signature: None,
            node_type: "section".to_string(),
        });
        edges.push(ExtractedEdge {
            source: file_id.clone(),
            target: section_id,
            relation: "contains".to_string(),
            confidence: "EXTRACTED".to_string(),
            confidence_score: Some(1.0),
            source_file: path.to_path_buf(),
            source_line: Some(start_line),
        });
    }

    Ok(Extraction {
        file_path: path.to_path_buf(),
        language: language.to_string(),
        nodes,
        edges,
    })
}

/// First `max` chars (byte-safe here: labels are trimmed ASCII-ish text and
/// clamped upstream) with an ellipsis suffix when longer.
fn truncate_with_ellipsis(text: &str, max: usize) -> String {
    if text.len() > max {
        format!("{}...", &text[..max - 3])
    } else {
        text.to_string()
    }
}

/// Extract structure from reStructuredText files (.rst).
pub(crate) fn extract_rst(path: &Path, naming: &Path) -> Result<Extraction, AstriaError> {
    let bytes = std::fs::read(path)?;
    let content = String::from_utf8_lossy(&bytes).into_owned();
    let fid = file_stem(naming);
    let file_id = make_node_id(&[&fid]);

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    nodes.push(ExtractedNode {
        id: file_id.clone(),
        label: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string(),
        source_file: path.to_path_buf(),
        source_line: None,
        docstring: None,
        signature: None,
        node_type: "document".to_string(),
    });

    let lines: Vec<&str> = content.lines().collect();
    let heading_chars: &[char] = &['=', '-', '~', '^', '"'];

    // Track heading nesting: stack of (underline_char, id)
    let mut heading_stack: Vec<(char, String)> = Vec::new();

    let mut i = 0;
    while i + 1 < lines.len() {
        let text_line = lines[i];
        let under_line = lines[i + 1];

        let trimmed_text = text_line.trim();
        let is_heading = !trimmed_text.is_empty()
            && !under_line.trim().is_empty()
            && under_line
                .trim()
                .chars()
                .all(|c| heading_chars.contains(&c))
            && under_line.trim().len() >= trimmed_text.len();

        if is_heading {
            let ch = under_line.trim().chars().next().unwrap_or('=');
            let title = trimmed_text.to_string();
            let slug = make_target_id(&title);
            let section_id = make_node_id(&[&fid, &slug]);

            nodes.push(ExtractedNode {
                id: section_id.clone(),
                label: title,
                source_file: path.to_path_buf(),
                source_line: Some(i as u32 + 1),
                docstring: None,
                signature: None,
                node_type: "section".to_string(),
            });

            // Pop stack until we find a parent with a different (higher-rank) char
            let char_rank = |c: char| heading_chars.iter().position(|&h| h == c).unwrap_or(0);
            while let Some((parent_ch, _)) = heading_stack.last() {
                if char_rank(*parent_ch) < char_rank(ch) {
                    break;
                }
                heading_stack.pop();
            }

            let parent_id = heading_stack
                .last()
                .map(|(_, id)| id.clone())
                .unwrap_or_else(|| file_id.clone());

            edges.push(ExtractedEdge {
                source: parent_id,
                target: section_id.clone(),
                relation: "contains".to_string(),
                confidence: "EXTRACTED".to_string(),
                confidence_score: Some(1.0),
                source_file: path.to_path_buf(),
                source_line: Some(i as u32 + 1),
            });

            heading_stack.push((ch, section_id));
            i += 2;
            continue;
        }
        i += 1;
    }

    Ok(Extraction {
        file_path: path.to_path_buf(),
        language: "rst".to_string(),
        nodes,
        edges,
    })
}
