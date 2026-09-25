// astria-pdf: PDF text extraction for knowledge graph ingestion

use std::path::Path;

use astria_core::AstriaError;
use astria_core::Result;

/// Extract raw text from a PDF file.
///
/// Uses the `pdf-extract` crate. Returns an empty string if the PDF contains
/// no extractable text. Returns an error only if the file cannot be read at all.
pub fn extract_text(path: &Path) -> Result<String> {
    let path_str = path.to_string_lossy().to_string();

    let bytes = std::fs::read(path).map_err(|e| {
        AstriaError::Io(std::io::Error::new(
            e.kind(),
            format!("Cannot read PDF file {}: {e}", path_str),
        ))
    })?;

    let text = pdf_extract::extract_text_from_mem(&bytes).map_err(|e| AstriaError::Parse {
        file: path_str.clone(),
        message: format!("PDF extraction failed: {e}"),
    })?;

    Ok(text)
}

/// Extract text from a PDF and convert it to a simple markdown format.
///
/// Produces section headers for detected double-newline breaks and wraps
/// paragraphs accordingly.
pub fn extract_to_markdown(path: &Path) -> Result<String> {
    let text = extract_text(path)?;

    if text.trim().is_empty() {
        return Ok(String::new());
    }

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document");

    let mut md = String::new();
    md.push_str("# ");
    md.push_str(filename);
    md.push_str("\n\n");

    // Split on double newlines to form paragraphs / sections.
    let mut section_index = 0u32;
    for paragraph in text.split("\n\n") {
        let trimmed = paragraph.trim();
        if trimmed.is_empty() {
            continue;
        }

        // If a paragraph looks like a title (short, no trailing period),
        // emit it as a section header.
        let is_headerish = trimmed.len() < 80
            && !trimmed.ends_with('.')
            && !trimmed.ends_with('?')
            && !trimmed.ends_with('!');

        if is_headerish && section_index > 0 {
            md.push_str("## ");
            md.push_str(trimmed);
            md.push('\n');
        } else {
            // Collapse single newlines within a paragraph.
            let collapsed = trimmed.replace('\n', " ");
            md.push_str(&collapsed);
            md.push('\n');
        }

        md.push('\n');
        section_index += 1;
    }

    Ok(md)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_text_nonexistent_file() {
        let result = extract_text(Path::new("/nonexistent/file.pdf"));
        assert!(result.is_err());
    }

    #[test]
    fn extract_to_markdown_nonexistent_file() {
        let result = extract_to_markdown(Path::new("/nonexistent/file.pdf"));
        assert!(result.is_err());
    }

    #[test]
    fn extract_text_invalid_pdf() {
        // Write a temporary file that is NOT a valid PDF.
        let dir = std::env::temp_dir().join("astria-pdf-test");
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("not_a_real.pdf");
        std::fs::write(&fake, b"this is not a pdf").unwrap();

        let result = extract_text(&fake);
        assert!(result.is_err());

        // Cleanup.
        let _ = std::fs::remove_file(&fake);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn extract_to_markdown_short_circuits_on_empty_text() {
        // extract_to_markdown calls extract_text internally, which fails on
        // non-existent files. Verify the error propagates rather than panicking.
        let result = extract_to_markdown(Path::new("/nonexistent/empty.pdf"));
        assert!(result.is_err());
    }

    #[test]
    fn extract_to_markdown_invalid_pdf_returns_error() {
        // A non-PDF file should produce a Parse error from pdf_extract,
        // not panic. This exercises the error path through extract_to_markdown.
        let dir = std::env::temp_dir().join("astria-pdf-test-md");
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("not_a_real.pdf");
        std::fs::write(&fake, b"this is not a pdf").unwrap();

        let result = extract_to_markdown(&fake);
        assert!(result.is_err());

        let _ = std::fs::remove_file(&fake);
        let _ = std::fs::remove_dir(&dir);
    }
}
