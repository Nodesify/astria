// docs: plain-text extractors for documentation formats (markdown, plain
// text, reStructuredText) — no tree-sitter involved.

use std::path::Path;

use crate::naming::{file_stem, make_node_id, make_text_id};
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
    // Body text between headings: the open section's slug, id, the 1-based
    // line its body starts at, and the accumulated lines. Text before the
    // first heading belongs to the document node.
    let mut pending: Option<(String, String, u32, String)> = None;
    let mut pre_body = String::new();
    let mut pre_body_start = 0u32;

    let flush_pending = |pending: &mut Option<(String, String, u32, String)>,
                         nodes: &mut Vec<ExtractedNode>,
                         edges: &mut Vec<ExtractedEdge>| {
        if let Some((section_id, _slug, start, body)) = pending.take() {
            if !body.trim().is_empty() {
                let parts: Vec<&str> = section_id.split("::").collect();
                let mut ctx = ChunkContext { nodes, edges, path };
                push_chunk_nodes(&mut ctx, &section_id, &parts, 0, &body, start);
            }
        }
    };

    for (line_no, line) in content.lines().enumerate() {
        // Parse headings
        if let Some(caps) = heading_re.captures(line) {
            flush_pending(&mut pending, &mut nodes, &mut edges);
            let hashes = caps.get(1).unwrap().as_str().len();
            let title = caps.get(2).unwrap().as_str().trim().to_string();
            let level = hashes;
            // A punctuation-only heading ("# ...") slugifies to empty, which
            // would make the section id equal the file node's id — a
            // guaranteed duplicate within the extraction.
            let slug = {
                let s = make_text_id(&title);
                if s.is_empty() {
                    "section".to_string()
                } else {
                    s
                }
            };
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

            heading_stack.push((level, section_id.clone()));
            pending = Some((section_id, slug, line_no as u32 + 2, String::new()));
            continue;
        }

        // Body text accumulates under the open section, or the document
        // before the first heading.
        match &mut pending {
            Some((_, _, _, body)) => {
                body.push_str(line);
                body.push('\n');
            }
            None => {
                if !line.trim().is_empty() {
                    if pre_body_start == 0 {
                        pre_body_start = line_no as u32 + 1;
                    }
                    pre_body.push_str(line);
                    pre_body.push('\n');
                }
            }
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
                let target_id = make_text_id(target_stem);
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
    flush_pending(&mut pending, &mut nodes, &mut edges);
    if !pre_body.trim().is_empty() {
        let mut ctx = ChunkContext {
            nodes: &mut nodes,
            edges: &mut edges,
            path,
        };
        push_chunk_nodes(
            &mut ctx,
            &file_id,
            &[&fid],
            0,
            &pre_body,
            pre_body_start.max(1),
        );
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
    extract_text_from_str(path, language, naming, &content)
}

/// Extract paragraphs from pre-read text (HTML tag stripping feeds this).
pub(crate) fn extract_text_from_str(
    path: &Path,
    language: &str,
    naming: &Path,
    content: &str,
) -> Result<Extraction, AstriaError> {
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

    let mut chunk_index = 0usize;
    let mut para_lines: Vec<&str> = Vec::new();
    let mut para_start = 0u32;

    let flush_paragraph = |para_lines: &mut Vec<&str>,
                           para_start: u32,
                           chunk_index: &mut usize,
                           nodes: &mut Vec<ExtractedNode>,
                           edges: &mut Vec<ExtractedEdge>| {
        if para_lines.is_empty() {
            return;
        }
        let para = para_lines.join("\n");
        para_lines.clear();
        if para.trim().is_empty() {
            return;
        }
        let before = nodes.len();
        let mut ctx = ChunkContext { nodes, edges, path };
        push_chunk_nodes(&mut ctx, &file_id, &[&fid], *chunk_index, &para, para_start);
        *chunk_index += nodes.len() - before;
    };

    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            flush_paragraph(
                &mut para_lines,
                para_start,
                &mut chunk_index,
                &mut nodes,
                &mut edges,
            );
        } else {
            if para_lines.is_empty() {
                para_start = i as u32 + 1;
            }
            para_lines.push(line);
        }
    }
    flush_paragraph(
        &mut para_lines,
        para_start,
        &mut chunk_index,
        &mut nodes,
        &mut edges,
    );

    Ok(Extraction {
        file_path: path.to_path_buf(),
        language: language.to_string(),
        nodes,
        edges,
    })
}

/// First `max` chars (split on a char boundary) with an ellipsis suffix
/// when longer.
fn truncate_with_ellipsis(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        let cut: usize = text
            .char_indices()
            .nth(max - 3)
            .map(|(i, _)| i)
            .unwrap_or(0);
        format!("{}...", &text[..cut])
    } else {
        text.to_string()
    }
}

/// Default maximum characters of body text carried by one chunk node
/// (~250-300 tokens): small enough that query-term scoring and embedding
/// inputs stay focused, large enough to keep doc-graph node counts sane.
/// Override per run with ASTRIA_CHUNK_CHARS (clamped to 400..=8000).
const CHUNK_MAX_CHARS: usize = 1200;
/// Characters of the previous chunk prepended to the next one so evidence
/// spanning a chunk boundary is still retrievable from either side.
const CHUNK_OVERLAP_CHARS: usize = 180;

fn chunk_max_chars() -> usize {
    chunk_max_chars_value(astria_core::env_var("CHUNK_CHARS").as_deref())
}

fn chunk_max_chars_value(value: Option<&str>) -> usize {
    value
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|v| v.clamp(400, 8000))
        .unwrap_or(CHUNK_MAX_CHARS)
}

/// A body-text chunk with the 1-based source line of its first line.
struct BodyChunk {
    start_line: u32,
    text: String,
}

/// Blank-line separated paragraphs, each pre-split so no piece exceeds
/// `max` characters — oversized paragraphs break at line, then hard
/// character boundaries.
fn paragraph_pieces(body: &str, max: usize) -> Vec<(u32, String)> {
    let mut pieces: Vec<(u32, String)> = Vec::new();
    let mut para_lines: Vec<&str> = Vec::new();
    let mut para_start = 0u32;
    let flush = |lines: &mut Vec<&str>, start: u32, pieces: &mut Vec<(u32, String)>| {
        if lines.is_empty() {
            return;
        }
        let para = lines.join("\n");
        lines.clear();
        if para.chars().count() <= max {
            if !para.trim().is_empty() {
                pieces.push((start, para));
            }
            return;
        }
        let mut buf = String::new();
        let mut buf_start = start;
        let mut offset = 0u32;
        for line in para.lines() {
            offset += 1;
            let line_chars = line.chars().count();
            if line_chars > max {
                if !buf.is_empty() {
                    pieces.push((buf_start, std::mem::take(&mut buf)));
                }
                let line_abs = start + offset - 1;
                let mut piece = String::new();
                for (i, c) in line.chars().enumerate() {
                    piece.push(c);
                    if (i + 1) % max == 0 {
                        pieces.push((line_abs, std::mem::take(&mut piece)));
                    }
                }
                if !piece.is_empty() {
                    pieces.push((line_abs, piece));
                }
                buf_start = line_abs;
                continue;
            }
            if buf.chars().count() + line_chars + 1 > max {
                pieces.push((buf_start, std::mem::take(&mut buf)));
                buf_start = start + offset - 1;
            }
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(line);
        }
        if !buf.trim().is_empty() {
            pieces.push((buf_start, buf));
        }
    };
    for (i, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            flush(&mut para_lines, para_start, &mut pieces);
            para_start = i as u32 + 1;
        } else {
            if para_lines.is_empty() {
                para_start = i as u32 + 1;
            }
            para_lines.push(line);
        }
    }
    flush(&mut para_lines, para_start, &mut pieces);
    pieces
}

/// Split body text into chunk-sized pieces at paragraph, then line, then
/// hard boundaries. `body_start` is the 1-based source line of the body's
/// first line; each chunk carries the absolute line it begins at.
fn chunk_body(body: &str, body_start: u32) -> Vec<BodyChunk> {
    let max = chunk_max_chars();
    let pieces = paragraph_pieces(body, max);
    let mut chunks: Vec<BodyChunk> = Vec::new();
    let mut current: Option<(u32, String)> = None;
    for (piece_line, piece) in pieces {
        match &mut current {
            Some((_, text)) if text.chars().count() + piece.chars().count() + 2 > max => {
                let (line, text) = current.take().unwrap();
                chunks.push(BodyChunk {
                    start_line: body_start + line - 1,
                    text,
                });
                current = Some((piece_line, piece));
            }
            Some((_, text)) => {
                text.push_str("\n\n");
                text.push_str(&piece);
            }
            None => current = Some((piece_line, piece)),
        }
    }
    if let Some((line, text)) = current {
        if !text.trim().is_empty() {
            chunks.push(BodyChunk {
                start_line: body_start + line - 1,
                text,
            });
        }
    }
    add_overlap(&mut chunks);
    chunks
}

/// Prepend a line-snapped tail of the previous chunk to each successor so a
/// match near a boundary surfaces from both chunks. `start_line` moves back
/// by the number of lines the tail occupies.
fn add_overlap(chunks: &mut [BodyChunk]) {
    for i in 1..chunks.len() {
        let tail = line_snapped_tail(&chunks[i - 1].text, CHUNK_OVERLAP_CHARS)
            .trim()
            .to_string();
        if tail.is_empty() {
            continue;
        }
        let tail_lines = tail.matches('\n').count() as u32 + 1;
        let text = chunks[i].text.clone();
        chunks[i].text = format!("{tail}\n{text}");
        chunks[i].start_line = chunks[i].start_line.saturating_sub(tail_lines - 1);
    }
}

/// Last `max` characters of `text`, cut at a line boundary when one falls
/// inside the window.
fn line_snapped_tail(text: &str, max: usize) -> String {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let cut = chars[chars.len() - max].0;
    let window = &text[cut..];
    match window.find('\n') {
        Some(pos) if pos + 1 < window.len() => window[pos + 1..].to_string(),
        _ => window.to_string(),
    }
}

/// Collector for chunk extraction, so callers do not thread eight separate
/// mutables through every document extractor.
struct ChunkContext<'a> {
    nodes: &'a mut Vec<ExtractedNode>,
    edges: &'a mut Vec<ExtractedEdge>,
    path: &'a Path,
}

/// Emit chunk nodes for `body` under `parent_id`, linked with `contains`
/// edges. Labels are each chunk's first line (truncated) so answers stay
/// readable; the docstring carries the full chunk text that query scoring
/// and embeddings search. `parent_parts` are the owning node's slug parts
/// (file id and any section slug), used to build chunk ids.
fn push_chunk_nodes(
    ctx: &mut ChunkContext,
    parent_id: &str,
    parent_parts: &[&str],
    first_index: usize,
    body: &str,
    body_start: u32,
) {
    for (i, chunk) in chunk_body(body, body_start).into_iter().enumerate() {
        let index_slug = format!("p{}", first_index + i);
        let id = make_node_id(
            &parent_parts
                .iter()
                .copied()
                .chain(std::iter::once(index_slug.as_str()))
                .collect::<Vec<_>>(),
        );
        let label = truncate_with_ellipsis(
            chunk
                .text
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("body"),
            80,
        );
        ctx.nodes.push(ExtractedNode {
            id: id.clone(),
            label,
            source_file: ctx.path.to_path_buf(),
            source_line: Some(chunk.start_line),
            docstring: Some(chunk.text),
            signature: None,
            node_type: "chunk".to_string(),
        });
        ctx.edges.push(ExtractedEdge {
            source: parent_id.to_string(),
            target: id,
            relation: "contains".to_string(),
            confidence: "EXTRACTED".to_string(),
            confidence_score: Some(1.0),
            source_file: ctx.path.to_path_buf(),
            source_line: Some(chunk.start_line),
        });
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
    // Repeated section titles in one file (e.g. changelogs with several
    // "API Changes" headings) get ordinal suffixes so ids stay unique —
    // same scheme as the markdown extractor.
    let mut seen_section_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Body text between headings, same scheme as the markdown extractor.
    let mut pending: Option<(String, String, u32, String)> = None;
    let mut pre_body = String::new();
    let mut pre_body_start = 0u32;

    let flush_pending = |pending: &mut Option<(String, String, u32, String)>,
                         nodes: &mut Vec<ExtractedNode>,
                         edges: &mut Vec<ExtractedEdge>| {
        if let Some((section_id, _slug, start, body)) = pending.take() {
            if !body.trim().is_empty() {
                let parts: Vec<&str> = section_id.split("::").collect();
                let mut ctx = ChunkContext { nodes, edges, path };
                push_chunk_nodes(&mut ctx, &section_id, &parts, 0, &body, start);
            }
        }
    };

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
            flush_pending(&mut pending, &mut nodes, &mut edges);
            let ch = under_line.trim().chars().next().unwrap_or('=');
            let title = trimmed_text.to_string();
            // Same empty-slug guard as the markdown extractor.
            let slug = {
                let s = make_text_id(&title);
                if s.is_empty() {
                    "section".to_string()
                } else {
                    s
                }
            };
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

            heading_stack.push((ch, section_id.clone()));
            pending = Some((section_id, slug, i as u32 + 3, String::new()));
            i += 2;
            continue;
        }

        match &mut pending {
            Some((_, _, _, body)) => {
                body.push_str(text_line);
                body.push('\n');
            }
            None => {
                if !text_line.trim().is_empty() {
                    if pre_body_start == 0 {
                        pre_body_start = i as u32 + 1;
                    }
                    pre_body.push_str(text_line);
                    pre_body.push('\n');
                }
            }
        }
        i += 1;
    }
    // A trailing line past the last (text, underline) pair still belongs to
    // the open section or the document preamble.
    if i < lines.len() {
        let last = lines[i];
        match &mut pending {
            Some((_, _, _, body)) => {
                body.push_str(last);
                body.push('\n');
            }
            None => {
                if !last.trim().is_empty() {
                    if pre_body_start == 0 {
                        pre_body_start = i as u32 + 1;
                    }
                    pre_body.push_str(last);
                    pre_body.push('\n');
                }
            }
        }
    }
    flush_pending(&mut pending, &mut nodes, &mut edges);
    if !pre_body.trim().is_empty() {
        let mut ctx = ChunkContext {
            nodes: &mut nodes,
            edges: &mut edges,
            path,
        };
        push_chunk_nodes(
            &mut ctx,
            &file_id,
            &[&fid],
            0,
            &pre_body,
            pre_body_start.max(1),
        );
    }

    Ok(Extraction {
        file_path: path.to_path_buf(),
        language: "rst".to_string(),
        nodes,
        edges,
    })
}

/// Extract an HTML file as text: scripts/styles are dropped, tags are
/// stripped, common entities are decoded, then the plain-text paragraph
/// chunker runs over what remains.
pub(crate) fn extract_html(path: &Path, naming: &Path) -> Result<Extraction, AstriaError> {
    let bytes = std::fs::read(path)?;
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    let text = strip_html(&raw);
    extract_text_from_str(path, "html", naming, &text)
}

static HTML_SCRIPTS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?is)<script\b[^>]*>.*?</script>").expect("static regex")
});
static HTML_STYLES: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?is)<style\b[^>]*>.*?</style>").expect("static regex")
});
static HTML_TAGS: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"(?s)<[^>]*>").expect("static regex"));

fn strip_html(input: &str) -> String {
    let text = HTML_SCRIPTS.replace_all(input, " ");
    let text = HTML_STYLES.replace_all(&text, " ");
    let text = HTML_TAGS.replace_all(&text, " ");
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_md(content: &str) -> Extraction {
        extract_markdown_from_string(
            Path::new("docs/notes.md"),
            "markdown",
            content,
            Path::new("docs/notes.md"),
        )
    }

    #[test]
    fn markdown_section_body_becomes_a_searchable_docstring() {
        let ex = extract_md(
            "# Title\n\nintro line\n\n## Setup\nInstall with npm install.\nThen run the build.\n",
        );
        let section = ex.nodes.iter().find(|n| n.label == "Setup").unwrap();
        let chunks: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.docstring.is_some() && n.id.starts_with(&section.id))
            .collect();
        assert_eq!(chunks.len(), 1);
        let doc = chunks[0].docstring.as_deref().unwrap();
        assert!(doc.contains("Install with npm install."));
        assert!(doc.contains("Then run the build."));
        assert_eq!(chunks[0].source_line, Some(6));
        assert!(chunks[0].id.ends_with("::p0"), "chunk id: {}", chunks[0].id);
    }

    #[test]
    fn heading_free_markdown_chunks_under_the_document() {
        let ex = extract_md("alpha paragraph with content\n\nbeta paragraph also\n");
        assert!(ex.nodes.iter().any(|n| n.node_type == "document"));
        let chunks: Vec<_> = ex.nodes.iter().filter(|n| n.node_type == "chunk").collect();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].docstring.as_deref().unwrap().contains("beta"));
        assert!(chunks[0].id.ends_with("::p0"));
    }

    #[test]
    fn long_bodies_split_at_chunk_boundaries() {
        let line = "x".repeat(300);
        let body: String = (0..8).map(|_| format!("{line}\n")).collect();
        let ex = extract_md(&format!("# Big\n\n{body}\n"));
        let chunks: Vec<_> = ex.nodes.iter().filter(|n| n.docstring.is_some()).collect();
        assert!(chunks.len() >= 2, "expected multiple chunks");
        for c in &chunks {
            assert!(
                c.docstring.as_deref().unwrap().chars().count() <= CHUNK_MAX_CHARS,
                "chunk exceeded the cap"
            );
        }
        let mut ids: Vec<_> = ex.nodes.iter().map(|n| n.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ex.nodes.len(), "chunk ids must stay unique");
    }

    #[test]
    fn text_paragraphs_keep_full_body() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("conv.txt");
        std::fs::write(
            &p,
            "first line has sunrise\nsecond line more sunrise\n\nother para\n",
        )
        .unwrap();
        let ex = extract_text_file(p.as_path(), "text", p.as_path()).unwrap();
        let chunks: Vec<_> = ex.nodes.iter().filter(|n| n.node_type == "chunk").collect();
        assert_eq!(chunks.len(), 2);
        let doc = chunks[0].docstring.as_deref().unwrap();
        assert!(doc.contains("first line has sunrise"));
        assert!(doc.contains("second line more sunrise"));
        assert_eq!(chunks[0].label, "first line has sunrise");
        assert!(chunks[0].id.ends_with("::p0"));
    }

    #[test]
    fn rst_section_body_is_captured() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("guide.rst");
        std::fs::write(
            &p,
            "Title\n=====\n\nSetup text here\nMore setup\n\nNext\n=====\n",
        )
        .unwrap();
        let ex = extract_rst(p.as_path(), p.as_path()).unwrap();
        let title = ex.nodes.iter().find(|n| n.label == "Title").unwrap();
        let chunks: Vec<_> = ex.nodes.iter().filter(|n| n.docstring.is_some()).collect();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0]
            .docstring
            .as_deref()
            .unwrap()
            .contains("Setup text here"));
        assert!(chunks[0].id.starts_with(&title.id));
    }

    #[test]
    fn rst_repeated_section_titles_get_unique_ids() {
        // Changelogs repeat section names ("API Changes" under several
        // releases); duplicate ids in one extraction are a validation error,
        // so the RST extractor must ordinal-suffix them like the markdown one.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("changelog.rst");
        std::fs::write(
            &p,
            "Changelog\n=========\n\n20.0\n----\n\nAPI Changes\n~~~~~~~~~~~\n\nFirst api body\n\n20.1\n----\n\nAPI Changes\n~~~~~~~~~~~\n\nSecond api body\n",
        )
        .unwrap();
        let ex = extract_rst(p.as_path(), p.as_path()).unwrap();
        let sections: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.label == "API Changes")
            .map(|n| n.id.clone())
            .collect();
        assert_eq!(sections.len(), 2, "both sections extracted");
        assert_ne!(
            sections[0], sections[1],
            "repeated titles must not collide: {sections:?}"
        );
    }

    #[test]
    fn rst_and_md_punctuation_headings_do_not_collide_with_file_node() {
        // "# ..." slugifies to the empty string; without a fallback the
        // section id equals the file node id and validation fails with a
        // duplicate node id in one extraction.
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("plugins.md");
        std::fs::write(
            &md,
            "# Plugins

body one

# ...

punct body

# ...

more punct
",
        )
        .unwrap();
        let ex = extract_markdown(md.as_path(), md.as_path()).unwrap();
        let ids: Vec<_> = ex.nodes.iter().map(|n| n.id.clone()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            ids.len(),
            sorted.len(),
            "duplicate ids in md extraction: {ids:?}"
        );

        let rst = dir.path().join("guide.rst");
        std::fs::write(
            &rst,
            "Guide
=====

intro

...
~~~~~

punct body
",
        )
        .unwrap();
        let ex2 = extract_rst(rst.as_path(), rst.as_path()).unwrap();
        let ids2: Vec<_> = ex2.nodes.iter().map(|n| n.id.clone()).collect();
        let mut s2 = ids2.clone();
        s2.sort();
        s2.dedup();
        assert_eq!(
            ids2.len(),
            s2.len(),
            "duplicate ids in rst extraction: {ids2:?}"
        );
    }

    #[test]
    fn chunks_overlap_across_boundaries() {
        let line = "x".repeat(300);
        let body: String = (0..8).map(|_| format!("{line}\n")).collect();
        let ex = extract_md(&format!("# Big\n\n{body}\n"));
        let chunks: Vec<_> = ex.nodes.iter().filter(|n| n.node_type == "chunk").collect();
        assert!(chunks.len() >= 2);
        // Each successor starts with the tail of its predecessor, so a match
        // spanning the boundary surfaces from either side.
        let first_text = chunks[0].docstring.as_deref().unwrap();
        let tail_of_first: String = first_text[first_text.len() - 40..].to_string();
        assert!(
            chunks[1]
                .docstring
                .as_deref()
                .unwrap()
                .contains(&tail_of_first),
            "successor chunk must carry the predecessor tail"
        );
        // The overlap shifts the reported start line back accordingly.
        assert!(chunks[1].source_line.unwrap() < chunks[0].source_line.unwrap() + 4);
        for c in &chunks {
            assert!(
                c.docstring.as_deref().unwrap().chars().count()
                    <= CHUNK_MAX_CHARS + CHUNK_OVERLAP_CHARS + 2,
                "chunk exceeded the cap including overlap"
            );
        }
    }

    #[test]
    fn overlap_tails_never_produce_empty_labels() {
        // Bodies whose paragraphs are separated by blank lines can snap an
        // overlap tail to a blank line; labels must still come from real
        // content or extraction validation fails.
        let body = "first paragraph line one".repeat(30)
            + "

" + &"second paragraph line".repeat(30)
            + "

" + &"third paragraph line".repeat(30)
            + "
";
        let ex = extract_md(&format!(
            "# Gap

{}
",
            body
        ));
        assert!(
            ex.nodes.iter().all(|n| !n.label.trim().is_empty()),
            "no chunk may carry an empty label"
        );
    }

    #[test]
    fn chunk_size_override_is_parsed_and_clamped() {
        assert_eq!(chunk_max_chars_value(None), CHUNK_MAX_CHARS);
        assert_eq!(chunk_max_chars_value(Some(" 2000 ")), 2000);
        assert_eq!(chunk_max_chars_value(Some("junk")), CHUNK_MAX_CHARS);
        assert_eq!(chunk_max_chars_value(Some("10")), 400, "clamped low");
        assert_eq!(chunk_max_chars_value(Some("999999")), 8000, "clamped high");
    }
}
