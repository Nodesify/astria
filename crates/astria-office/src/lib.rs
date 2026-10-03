// astria-office: .docx/.xlsx extraction for knowledge graph ingestion
//
// Mirrors astria-pdf's shape: read an Office Open XML file and return simple
// markdown that the engine then parses with the regular document chunker.
// Word documents become headings/paragraphs/lists/tables; workbooks become
// one markdown table per sheet. Everything is local and deterministic.

use std::path::Path;

use astria_core::{AstriaError, Result};

/// Sanity caps so a pathological workbook cannot explode the graph.
const MAX_SHEET_ROWS: usize = 500;
const MAX_SHEET_COLS: usize = 30;

/// Extract a `.docx` or `.xlsx` file as simple markdown.
pub fn extract_to_markdown(path: &Path) -> Result<String> {
    let path_str = path.to_string_lossy().to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let bytes = std::fs::read(path).map_err(|e| {
        AstriaError::Io(std::io::Error::new(
            e.kind(),
            format!("Cannot read Office file {}: {e}", path_str),
        ))
    })?;

    match ext.as_str() {
        "docx" => docx_to_markdown(&bytes).map_err(|e| AstriaError::Parse {
            file: path_str,
            message: format!("DOCX extraction failed: {e}"),
        }),
        "xlsx" => xlsx_to_markdown(&bytes).map_err(|e| AstriaError::Parse {
            file: path_str,
            message: format!("XLSX extraction failed: {e}"),
        }),
        other => Err(AstriaError::Parse {
            file: path_str,
            message: format!("astria-office: unsupported extension .{other}"),
        }),
    }
}

// ---------------------------------------------------------------------------
// DOCX
// ---------------------------------------------------------------------------

/// Convert the main document part of a .docx (a zip containing
/// `word/document.xml`) into markdown: `#`-prefixed headings for
/// Heading1-3/Title styles, `- ` for list paragraphs, and `|`-delimited
/// rows for tables.
fn docx_to_markdown(bytes: &[u8]) -> std::result::Result<String, String> {
    let xml = read_zip_part(bytes, "word/document.xml")?;

    let mut reader = quick_xml::Reader::from_str(&xml);
    reader.config_mut().trim_text(false);

    let mut md = String::new();
    // Paragraph state
    let mut para_text = String::new();
    let mut para_heading = 0u8; // 0 = not a heading, else level 1..=3
    let mut para_list = false;
    // Table state
    let mut in_cell = false;
    let mut cell_text = String::new();
    let mut row_cells: Vec<String> = Vec::new();
    let mut first_row_of_table = false;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) => match e.local_name().as_ref() {
                b"p" => {
                    para_text.clear();
                    para_heading = 0;
                    para_list = false;
                }
                b"pStyle" => {
                    if let Some(val) = attr(&e, b"val") {
                        para_heading = heading_level(&val);
                    }
                }
                b"numPr" => para_list = true,
                b"tbl" => first_row_of_table = true,
                b"tr" => row_cells.clear(),
                b"tc" => {
                    in_cell = true;
                    cell_text.clear();
                }
                b"tab" => push_space(&mut para_text, in_cell, &mut cell_text),
                b"br" | b"cr" => push_space(&mut para_text, in_cell, &mut cell_text),
                _ => {}
            },
            Ok(quick_xml::events::Event::Empty(e)) => match e.local_name().as_ref() {
                b"tab" | b"br" | b"cr" => {
                    push_space(&mut para_text, in_cell, &mut cell_text);
                }
                b"pStyle" => {
                    if let Some(val) = attr(&e, b"val") {
                        para_heading = heading_level(&val);
                    }
                }
                b"numPr" => para_list = true,
                _ => {}
            },
            Ok(quick_xml::events::Event::Text(t)) => {
                let decoded = t.unescape().map_err(|e| e.to_string())?;
                if in_cell {
                    cell_text.push_str(&decoded);
                } else {
                    para_text.push_str(&decoded);
                }
            }
            Ok(quick_xml::events::Event::End(e)) => match e.local_name().as_ref() {
                b"p" => {
                    flush_paragraph(&mut md, &para_text, para_heading, para_list);
                    para_text.clear();
                    para_heading = 0;
                    para_list = false;
                }
                b"tc" => {
                    row_cells.push(cell_text.trim().to_string());
                    cell_text.clear();
                    in_cell = false;
                }
                b"tr" => {
                    flush_row(&mut md, &row_cells, first_row_of_table);
                    first_row_of_table = false;
                    row_cells.clear();
                }
                _ => {}
            },
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(e.to_string()),
            Ok(_) => {}
        }
        buf.clear();
    }

    Ok(md)
}

fn push_space(para_text: &mut String, in_cell: bool, cell_text: &mut String) {
    if in_cell {
        cell_text.push(' ');
    } else {
        para_text.push(' ');
    }
}

fn flush_paragraph(md: &mut String, para_text: &str, heading: u8, list: bool) {
    let text = para_text.trim();
    if text.is_empty() {
        return;
    }
    if heading > 0 {
        md.push_str(&"#".repeat(heading as usize));
        md.push(' ');
        md.push_str(text);
    } else if list {
        md.push_str("- ");
        md.push_str(text);
    } else {
        md.push_str(text);
    }
    md.push('\n');
}

fn flush_row(md: &mut String, row_cells: &[String], first_row: bool) {
    if row_cells.is_empty() {
        return;
    }
    md.push_str("| ");
    md.push_str(&row_cells.join(" | "));
    md.push_str(" |\n");
    if first_row {
        let sep = vec!["---"; row_cells.len()].join(" | ");
        md.push_str("| ");
        md.push_str(&sep);
        md.push_str(" |\n");
    }
}

// ---------------------------------------------------------------------------
// XLSX
// ---------------------------------------------------------------------------

/// Convert a .xlsx workbook into markdown: one `## <sheet>` section per
/// non-empty sheet with a markdown table (first row = header). Rows beyond
/// [`MAX_SHEET_ROWS`] and columns beyond [`MAX_SHEET_COLS`] are dropped.
fn xlsx_to_markdown(bytes: &[u8]) -> std::result::Result<String, String> {
    use calamine::Reader;

    // Bounds pre-scan: each worksheet part's DECLARED used range must fit
    // the extraction bounds before calamine materializes anything. The
    // compressed input can hide a much larger sheet; `worksheet_range`
    // allocates the full declared grid up front, so the post-load guard
    // alone would fire after the memory was already spent.
    xlsx_dimensions_within_bounds(bytes)?;

    // The compressed input can hide a much larger sheet; refuse workbooks
    // whose sheets declare more cells than the output caps could ever need
    // before loading the range into memory.
    let cursor = std::io::Cursor::new(bytes);
    let mut workbook = calamine::Xlsx::new(cursor).map_err(|e| e.to_string())?;

    let mut md = String::new();
    for name in workbook.sheet_names() {
        let Ok(range) = workbook.worksheet_range(&name) else {
            continue;
        };
        // Belt and braces: a sheet without a usable declared dimension (or
        // one that lied) is still bounded after loading.
        let (rows, cols) = range.get_size();
        if rows > MAX_SHEET_ROWS.saturating_mul(4) || cols > MAX_SHEET_COLS.saturating_mul(4) {
            return Err(format!(
                "sheet {name} loads {rows}x{cols} cells, beyond the extraction bounds \
                 ({} rows x {} cols)",
                MAX_SHEET_ROWS, MAX_SHEET_COLS
            ));
        }
        let mut emitted_header = false;
        let mut row_count = 0usize;
        for row in range.rows() {
            let cells: Vec<String> = row
                .iter()
                .take(MAX_SHEET_COLS)
                .map(cell_to_string)
                .collect();
            if cells.iter().all(|c| c.is_empty()) {
                continue;
            }
            if !emitted_header {
                md.push_str("## ");
                md.push_str(&name);
                md.push_str("\n\n");
                emitted_header = true;
            }
            md.push_str("| ");
            md.push_str(&cells.join(" | "));
            md.push_str(" |\n");
            if row_count == 0 {
                let sep = vec!["---"; cells.len()].join(" | ");
                md.push_str("| ");
                md.push_str(&sep);
                md.push_str(" |\n");
            }
            row_count += 1;
            if row_count >= MAX_SHEET_ROWS {
                break;
            }
        }
    }
    Ok(md)
}

fn cell_to_string(cell: &calamine::Data) -> String {
    match cell {
        calamine::Data::Empty => String::new(),
        calamine::Data::String(s) => s.replace('|', "\\|"),
        other => other.to_string(),
    }
}

/// Scan every worksheet part (`xl/worksheets/*.xml`) for its declared used
/// range (`<dimension ref="A1:BZ999"/>`) and refuse the workbook when any
/// sheet declares more rows/columns than the extraction bounds allow. The
/// scan reads each part under [`MAX_PART_BYTES`], so it cannot be blown up
/// by a hostile archive either.
fn xlsx_dimensions_within_bounds(bytes: &[u8]) -> std::result::Result<(), String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;
    for index in 0..archive.len() {
        let mut part = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = part.name().to_string();
        if !(name.starts_with("xl/worksheets/") && name.ends_with(".xml")) {
            continue;
        }
        if part.size() > MAX_PART_BYTES {
            return Err(format!(
                "worksheet part {name} expands to {} bytes (limit {MAX_PART_BYTES})",
                part.size()
            ));
        }
        let mut xml = String::new();
        let mut limited = std::io::Read::take(&mut part, MAX_PART_BYTES);
        std::io::Read::read_to_string(&mut limited, &mut xml).map_err(|e| e.to_string())?;
        if let Some((rows, cols)) = declared_dimension(&xml) {
            if rows > MAX_SHEET_ROWS.saturating_mul(4) || cols > MAX_SHEET_COLS.saturating_mul(4) {
                return Err(format!(
                    "sheet {name} declares {rows}x{cols} cells, beyond the extraction bounds \
                     ({MAX_SHEET_ROWS} rows x {MAX_SHEET_COLS} cols)"
                ));
            }
        }
    }
    Ok(())
}

/// The sheet's declared extent (`rows x cols`) from its
/// `<dimension ref="A1:C5"/>` element. `None` when the part declares no
/// dimension or an unusable one (full-column/full-row refs like `A:A` carry
/// no row count — the post-load guard covers those).
fn declared_dimension(xml: &str) -> Option<(usize, usize)> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e))
                if e.local_name().as_ref() == b"dimension" =>
            {
                let ref_attr = attr(&e, b"ref")?;
                return declared_extent(&ref_attr);
            }
            Ok(quick_xml::events::Event::Eof) => return None,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
}

/// Extent of an A1-style range: `A1:C5` → (5 rows, 3 cols); a single cell
/// `B7` → (1, 1). Absolute `$` markers are tolerated; refs missing either
/// axis (`A:A`, `1:3`) are unusable → `None`.
fn declared_extent(ref_attr: &str) -> Option<(usize, usize)> {
    let clean = |s: &str| s.replace('$', "");
    let (start, end) = match clean(ref_attr).split_once(':') {
        Some((s, e)) => (s.to_string(), e.to_string()),
        None => {
            let single = clean(ref_attr);
            return cell_axes(&single).map(|_| (1usize, 1usize));
        }
    };
    let (start_row, start_col) = cell_axes(&start)?;
    let (end_row, end_col) = cell_axes(&end)?;
    Some((
        end_row.checked_sub(start_row)?.checked_add(1)?,
        end_col.checked_sub(start_col)?.checked_add(1)?,
    ))
}

/// (row, col) of an A1-style cell reference, both 1-based.
fn cell_axes(cell: &str) -> Option<(usize, usize)> {
    let bytes = cell.as_bytes();
    let letters: Vec<u8> = bytes
        .iter()
        .copied()
        .take_while(|b| b.is_ascii_alphabetic())
        .collect();
    let digits: Vec<u8> = bytes
        .iter()
        .copied()
        .skip(letters.len())
        .take_while(|b| b.is_ascii_digit())
        .collect();
    if letters.is_empty() || digits.is_empty() || letters.len() + digits.len() != cell.len() {
        return None; // not a plain A1 cell (column-only/row-only ref, etc.)
    }
    if letters.len() > 3 {
        return None; // beyond XFD (16384), Excel's own maximum
    }
    let mut col = 0usize;
    for letter in letters.iter().map(|b| (*b as char).to_ascii_uppercase()) {
        let digit = letter as usize - 'A' as usize + 1; // A=1 ..= Z=26
        col = col.checked_mul(26)?.checked_add(digit)?;
    }
    let row: usize = std::str::from_utf8(&digits).ok()?.parse().ok()?;
    Some((row, col))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Hard cap on one decompressed ZIP part. The discovery size limit applies
/// to the COMPRESSED file; a small, highly compressed input must not
/// expand into an unbounded in-memory string (zip bomb).
const MAX_PART_BYTES: u64 = 64 * 1024 * 1024;

/// Read one part out of a zip archive (in memory), bounded: the declared
/// decompressed size is checked up front and the read is capped regardless
/// of what the header claims.
fn read_zip_part(bytes: &[u8], name: &str) -> std::result::Result<String, String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;
    let mut file = archive
        .by_name(name)
        .map_err(|e| format!("missing {name} in document: {e}"))?;
    if file.size() > MAX_PART_BYTES {
        return Err(format!(
            "document part {name} expands to {} bytes (limit {MAX_PART_BYTES})",
            file.size()
        ));
    }
    let mut xml = String::new();
    // take() bounds the read even when the declared size lied.
    let mut limited = std::io::Read::take(&mut file, MAX_PART_BYTES);
    std::io::Read::read_to_string(&mut limited, &mut xml).map_err(|e| e.to_string())?;
    Ok(xml)
}

/// Map OOXML paragraph style names to heading levels 1-3 (0 = not a heading).
fn heading_level(style: &str) -> u8 {
    match style {
        "Title" | "Heading1" => 1,
        "Heading2" => 2,
        "Heading3" => 3,
        _ => 0,
    }
}

/// Read one attribute (local name, namespace-agnostic) off a Start tag.
fn attr(event: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    for a in event.attributes().flatten() {
        if a.key.local_name().as_ref() == name {
            return a.unescape_value().ok().map(|v| v.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build an in-memory zip with the given (name, content) parts.
    fn zip_of(parts: &[(&str, String)]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        for (name, content) in parts {
            zip.start_file(*name, options.clone()).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn docx_with(document_xml: &str) -> Vec<u8> {
        zip_of(&[("word/document.xml", document_xml.to_string())])
    }

    #[test]
    fn docx_headings_paragraphs_and_lists() {
        let doc = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Project Plan</w:t></w:r></w:p>
    <w:p><w:r><w:t>Plain paragraph text.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Overview</w:t></w:r></w:p>
    <w:p><w:pPr><w:numPr/></w:pPr><w:r><w:t>First item</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Details</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
        let md = docx_to_markdown(&docx_with(doc)).unwrap();
        assert!(md.contains("# Project Plan\n"), "title heading: {md}");
        assert!(md.contains("Plain paragraph text.\n"));
        assert!(md.contains("# Overview\n"), "heading1: {md}");
        assert!(md.contains("- First item\n"), "list item: {md}");
        assert!(md.contains("## Details\n"), "heading2: {md}");
    }

    #[test]
    fn docx_tables_become_markdown_tables() {
        let doc = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:tbl>
      <w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Role</w:t></w:r></w:p></w:tc></w:tr>
      <w:tr><w:tc><w:p><w:r><w:t>Alice</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Owner</w:t></w:r></w:p></w:tc></w:tr>
    </w:tbl>
  </w:body>
</w:document>"#;
        let md = docx_to_markdown(&docx_with(doc)).unwrap();
        assert!(md.contains("| Name | Role |\n"), "header row: {md}");
        assert!(md.contains("| --- | --- |\n"), "separator: {md}");
        assert!(md.contains("| Alice | Owner |\n"), "data row: {md}");
    }

    #[test]
    fn docx_tabs_and_breaks_collapse_to_spaces() {
        let doc = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:r><w:t>left</w:t></w:r><w:tab/><w:r><w:t>right</w:t></w:r></w:p></w:body>
</w:document>"#;
        let md = docx_to_markdown(&docx_with(doc)).unwrap();
        assert!(md.contains("left right\n"), "tab collapsed: {md:?}");
    }

    #[test]
    fn docx_empty_document_yields_empty_markdown() {
        let doc = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#;
        let md = docx_to_markdown(&docx_with(doc)).unwrap();
        assert!(md.trim().is_empty());
    }

    #[test]
    fn docx_missing_document_part_is_an_error() {
        let bytes = zip_of(&[("other.xml", "<x/>".to_string())]);
        assert!(docx_to_markdown(&bytes).is_err());
    }

    #[test]
    fn extract_to_markdown_routes_by_extension() {
        let dir = tempfile::tempdir().unwrap();
        let docx = dir.path().join("sample.docx");
        std::fs::write(
            &docx,
            docx_with(
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Hello office</w:t></w:r></w:p></w:body></w:document>"#,
            ),
        )
        .unwrap();
        let md = extract_to_markdown(&docx).unwrap();
        assert!(md.contains("Hello office"));

        let txt = dir.path().join("sample.txt");
        std::fs::write(&txt, b"plain").unwrap();
        assert!(extract_to_markdown(&txt).is_err(), "unsupported extension");

        assert!(extract_to_markdown(Path::new("/nonexistent/file.docx")).is_err());
    }

    #[test]
    fn xlsx_sheets_become_markdown_tables() {
        let bytes = minimal_xlsx(&[
            (
                "Sheet1",
                &[&["Name", "Qty"], &["Bolt", "12"], &["Nut", "5"]],
            ),
            ("Empty", &[&["a"]]),
        ]);
        let md = xlsx_to_markdown(&bytes).unwrap();
        assert!(md.contains("## Sheet1\n"), "sheet heading: {md}");
        assert!(md.contains("| Name | Qty |\n"), "header: {md}");
        assert!(md.contains("| --- | --- |\n"));
        assert!(md.contains("| Bolt | 12 |\n"), "row: {md}");
        assert!(md.contains("| Nut | 5 |\n"));
        assert!(md.contains("## Empty\n"), "second sheet: {md}");
    }

    #[test]
    fn xlsx_small_sheet_smoke() {
        let bytes = minimal_xlsx(&[("S", &[&["a", "b", "c", "d"], &["1", "2", "3", "4"]])]);
        let md = xlsx_to_markdown(&bytes).unwrap();
        assert!(md.contains("| a | b | c | d |\n"));
        assert!(md.contains("| 1 | 2 | 3 | 4 |\n"));
    }

    #[test]
    fn xlsx_corrupt_bytes_are_an_error() {
        assert!(xlsx_to_markdown(b"not a zip").is_err());
    }

    /// Hand-build a minimal (but valid) xlsx: the same zip-of-XML layout
    /// Excel-compatible tools emit, with inline strings so no shared string
    /// table is needed.
    fn minimal_xlsx(sheets: &[(&str, &[&[&str]])]) -> Vec<u8> {
        let xml_decl = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

        let mut content_types = format!(
            r#"{xml_decl}
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#
        );
        let mut workbook = format!(
            r#"{xml_decl}
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#
        );
        let rels = format!(
            r#"{xml_decl}
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
        );
        let mut wb_rels = format!(
            r#"{xml_decl}
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#
        );

        let mut parts: Vec<(String, String)> = vec![
            ("[Content_Types].xml".into(), String::new()),
            ("_rels/.rels".into(), rels),
            ("xl/workbook.xml".into(), String::new()),
            ("xl/_rels/workbook.xml.rels".into(), String::new()),
        ];

        for (i, (name, rows)) in sheets.iter().enumerate() {
            let rid = format!("rId{}", i + 1);
            content_types.push_str(&format!(
                "\n<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>",
                i + 1
            ));
            workbook.push_str(&format!(
                "\n<sheet name=\"{}\" sheetId=\"{}\" r:id=\"{}\"/>",
                name,
                i + 1,
                rid
            ));
            wb_rels.push_str(&format!(
                "\n<Relationship Id=\"{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{}.xml\"/>",
                rid,
                i + 1
            ));

            let mut sheet = format!(
                r#"{xml_decl}
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#
            );
            for (r, row) in rows.iter().enumerate() {
                sheet.push_str(&format!("\n<row r=\"{}\">", r + 1));
                for (c, cell_value) in row.iter().enumerate() {
                    let col = col_letter(c);
                    sheet.push_str(&format!(
                        "<c r=\"{}{}\" t=\"inlineStr\"><is><t>{}</t></is></c>",
                        col,
                        r + 1,
                        xml_escape(cell_value)
                    ));
                }
                sheet.push_str("</row>");
            }
            sheet.push_str("\n</sheetData></worksheet>");
            parts.push((format!("xl/worksheets/sheet{}.xml", i + 1), sheet));
        }

        content_types.push_str("\n</Types>");
        workbook.push_str("\n</sheets></workbook>");
        wb_rels.push_str("\n</Relationships>");

        parts[0].1 = content_types;
        parts[2].1 = workbook;
        parts[3].1 = wb_rels;

        let refs: Vec<(&str, String)> =
            parts.iter().map(|(n, c)| (n.as_str(), c.clone())).collect();
        zip_of(&refs)
    }

    #[test]
    fn declared_extent_parses_a1_ranges() {
        assert_eq!(declared_extent("A1:C5"), Some((5, 3)));
        assert_eq!(declared_extent("B7"), Some((1, 1)));
        assert_eq!(declared_extent("$A$1:$BZ$100"), Some((100, 78)));
        // Full-column / full-row refs carry no usable row/col count.
        assert_eq!(declared_extent("A:A"), None);
        assert_eq!(declared_extent("1:3"), None);
        assert_eq!(declared_extent(""), None);
    }

    #[test]
    fn cell_axes_rejects_non_cell_refs() {
        assert_eq!(cell_axes("A1"), Some((1, 1)));
        assert_eq!(cell_axes("AA12"), Some((12, 27)));
        assert_eq!(cell_axes("XFD1048576"), Some((1048576, 16384)));
        assert_eq!(cell_axes("A"), None);
        assert_eq!(cell_axes("A:"), None);
        assert_eq!(cell_axes("12"), None);
        assert_eq!(cell_axes("AAAA1"), None, "beyond Excel's own max column");
    }

    #[test]
    fn oversized_declared_dimension_is_refused_before_loading() {
        // A sheet that DECLARES a 1048576x16384 used range but carries no
        // cells: the pre-scan must refuse it before calamine materializes
        // the declared grid.
        let xml_decl = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
        let sheet = format!(
            r#"{xml_decl}<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:XFD1048576"/><sheetData/></worksheet>"#
        );
        let bytes = zip_of(&[("xl/worksheets/sheet1.xml", sheet)]);
        let err = xlsx_to_markdown(&bytes).unwrap_err();
        assert!(
            err.contains("declares"),
            "message should name the declaration: {err}"
        );
    }

    #[test]
    fn modest_declared_dimension_passes_the_prescan() {
        let xml_decl = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
        let sheet = format!(
            r#"{xml_decl}<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:C3"/><sheetData></sheetData></worksheet>"#
        );
        let bytes = zip_of(&[("xl/worksheets/sheet1.xml", sheet)]);
        // The pre-scan itself is the unit under test here: a modest
        // declared dimension passes it. (Full extraction needs the
        // workbook scaffolding the sheets tests above already cover.)
        assert!(xlsx_dimensions_within_bounds(&bytes).is_ok());
    }

    fn col_letter(mut col: usize) -> String {
        let mut out = String::new();
        loop {
            let rem = col % 26;
            out.insert(0, (b'A' + rem as u8) as char);
            if col < 26 {
                break;
            }
            col = col / 26 - 1;
        }
        out
    }

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }
}
