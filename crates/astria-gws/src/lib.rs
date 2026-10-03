// astria-gws: Google Workspace shortcut ingestion (.gdoc/.gsheet/.gslides)
//
// Drive desktop/backup-and-sync shortcuts are tiny files that point at a
// cloud document. We resolve the Drive file id, export the document via the
// Drive API v3 `files.export` endpoint, and return markdown for the regular
// document chunker.
//
// Failure semantics mirror the whisper-cli integration: missing credentials
// or a failed export degrade to an actionable notice and an UNcached empty
// extraction, never a pipeline failure.

use std::path::Path;

const DRIVE_EXPORT_URL: &str = "https://www.googleapis.com/drive/v3/files/";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const ADC_RELATIVE: &[&str] = &[
    // POSIX gcloud ADC path, Windows %APPDATA%\gcloud\... handled below
    ".config/gcloud/application_default_credentials.json",
    "gcloud/application_default_credentials.json",
];

#[derive(Debug)]
pub enum GwsError {
    /// Missing tooling/credentials - skip with an actionable notice.
    Unavailable(String),
    /// The shortcut resolved but the export failed - warn and skip.
    Failed(String),
}

impl std::fmt::Display for GwsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GwsError::Unavailable(n) => write!(f, "{n}"),
            GwsError::Failed(m) => write!(f, "{m}"),
        }
    }
}

/// Export a `.gdoc` / `.gsheet` / `.gslides` shortcut file as markdown.
pub fn export_to_markdown(path: &Path) -> std::result::Result<String, GwsError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = export_mime(&ext).ok_or_else(|| {
        GwsError::Failed(format!("unsupported Google Workspace extension .{ext}"))
    })?;

    let bytes = std::fs::read(path)
        .map_err(|e| GwsError::Failed(format!("cannot read {}: {e}", path.display())))?;
    let text = String::from_utf8_lossy(&bytes);
    let file_id = resolve_file_id(&text).ok_or_else(|| {
        GwsError::Failed(format!(
            "no Drive file id found in {} (expected a docs.google.com / drive.google.com link)",
            path.display()
        ))
    })?;

    // Resolved at runtime from ASTRIA_GDRIVE_ACCESS_TOKEN or gcloud ADC -
    // nothing secret is ever embedded in source.
    let bearer = resolve_access_token().map_err(GwsError::Unavailable)?;

    let body = http_get_export(&file_id, mime, &bearer)?;

    match ext.as_str() {
        "gsheet" => Ok(csv_to_markdown(&body)),
        _ => Ok(body),
    }
}

/// Map a Google Workspace extension to its Drive export MIME type.
pub fn export_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "gdoc" => Some("text/plain"),
        "gsheet" => Some("text/csv"),
        "gslides" => Some("text/plain"),
        _ => None,
    }
}

/// Find a Drive file id in a shortcut file: either a JSON `url` field or any
/// docs/drive link embedded in the text.
pub fn resolve_file_id(text: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) {
        if let Some(url) = value.get("url").and_then(|u| u.as_str()) {
            if let Some(id) = id_from_url(url) {
                return Some(id.to_string());
            }
        }
    }
    id_from_url(text).map(|s| s.to_string())
}

/// Extract the document id from a Docs/Sheets/Slides/Drive URL.
pub fn id_from_url(haystack: &str) -> Option<&str> {
    for marker in [
        "/document/d/",
        "/spreadsheets/d/",
        "/presentation/d/",
        "/file/d/",
        "/drive/folders/",
        "id=",
    ] {
        if let Some(pos) = haystack.find(marker) {
            let rest = &haystack[pos + marker.len()..];
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if !id.is_empty() {
                return Some(Box::leak(id.into_boxed_str()));
            }
        }
    }
    None
}

/// Resolve a Drive API access token: env bearer first, then gcloud ADC
/// refresh-token flow. Both absent -> Unavailable with actionable text.
pub fn resolve_access_token() -> std::result::Result<String, String> {
    if let Ok(tok) = std::env::var("ASTRIA_GDRIVE_ACCESS_TOKEN") {
        if !tok.trim().is_empty() {
            return Ok(tok.trim().to_string());
        }
    }
    if let Ok(json_path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        if let Some(tok) = refresh_via_adc_file(&json_path) {
            return Ok(tok);
        }
    }
    for rel in ADC_RELATIVE {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let mut p = std::path::PathBuf::from(&home);
            for part in rel.split('/') {
                p.push(part);
            }
            if let Some(tok) = refresh_via_adc_file(&p.to_string_lossy()) {
                return Ok(tok);
            }
            if *rel == "gcloud/application_default_credentials.json" {
                if let Some(appdata) = std::env::var_os("APPDATA") {
                    let mut p2 = std::path::PathBuf::from(&appdata);
                    for part in rel.split('/') {
                        p2.push(part);
                    }
                    if let Some(tok) = refresh_via_adc_file(&p2.to_string_lossy()) {
                        return Ok(tok);
                    }
                }
            }
        }
    }
    Err(
        "Google Drive credentials not found - set ASTRIA_GDRIVE_ACCESS_TOKEN or run \
         `gcloud auth application-default login` (Drive read-only scope)"
            .to_string(),
    )
}

/// Minimal CSV -> markdown table (Drive Sheets CSV export).
pub fn csv_to_markdown(csv: &str) -> String {
    let mut md = String::new();
    let mut first = true;
    for line in csv.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let cells: Vec<String> = split_csv_line(line);
        md.push_str("| ");
        md.push_str(&cells.join(" | "));
        md.push_str(
            " |
",
        );
        if first {
            let sep = vec!["---"; cells.len()].join(" | ");
            md.push_str("| ");
            md.push_str(&sep);
            md.push_str(
                " |
",
            );
            first = false;
        }
    }
    md
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(c);
            }
        } else {
            match c {
                '"' => in_quotes = true,
                ',' => {
                    cells.push(current.trim().to_string());
                    current.clear();
                }
                _ => current.push(c),
            }
        }
    }
    cells.push(current.trim().to_string());
    cells
}

/// Shared agent: bounded global timeout, no cross-request state.
fn gws_agent() -> ureq::Agent {
    ureq::config::Config::builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .new_agent()
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn refresh_via_adc_file(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    if value.get("type").and_then(|t| t.as_str()) != Some("authorized_user") {
        return None;
    }
    let refresh = value.get("refresh_token")?.as_str()?;
    let client_id = value.get("client_id")?.as_str()?;
    let client_secret_value = value.get("client_secret")?.as_str()?;

    let form = format!(
        "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
        urlencode(client_id),
        urlencode(client_secret_value),
        urlencode(refresh)
    );
    let response = gws_agent()
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(form.as_str())
        .ok()?;
    let mut body = String::new();
    std::io::Read::read_to_string(&mut response.into_body().into_reader(), &mut body).ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    json.get("access_token")?.as_str().map(|s| s.to_string())
}

fn http_get_export(
    file_id: &str,
    mime: &str,
    bearer: &str,
) -> std::result::Result<String, GwsError> {
    let url = format!("{DRIVE_EXPORT_URL}{file_id}/export?mimeType={mime}");
    let response = gws_agent()
        .get(&url)
        .header("Authorization", &format!("Bearer {bearer}"))
        .call()
        .map_err(|e| GwsError::Failed(format!("Drive export failed for {file_id}: {e}")))?;
    if !response.status().is_success() {
        return Err(GwsError::Failed(format!(
            "Drive export failed for {file_id}: HTTP {} (check the link is shared with the authenticated account)",
            response.status()
        )));
    }
    let mut body = String::new();
    std::io::Read::read_to_string(&mut response.into_body().into_reader(), &mut body)
        .map_err(|e| GwsError::Failed(format!("Drive export body read failed: {e}")))?;
    Ok(body)
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_mapping_covers_all_three_extensions() {
        assert_eq!(export_mime("gdoc"), Some("text/plain"));
        assert_eq!(export_mime("gsheet"), Some("text/csv"));
        assert_eq!(export_mime("gslides"), Some("text/plain"));
        assert_eq!(export_mime("docx"), None);
    }

    #[test]
    fn file_id_from_shortcut_json() {
        let shortcut =
            r#"{"url": "https://docs.google.com/document/d/1AbC_dEf-123/edit?usp=drivesdk"}"#;
        assert_eq!(resolve_file_id(shortcut).as_deref(), Some("1AbC_dEf-123"));
    }

    #[test]
    fn file_id_from_bare_link_text() {
        assert_eq!(
            resolve_file_id("https://docs.google.com/spreadsheets/d/XYZ_9/edit#gid=0").as_deref(),
            Some("XYZ_9")
        );
        assert_eq!(
            resolve_file_id("see https://drive.google.com/file/d/1a2b3c/view please").as_deref(),
            Some("1a2b3c")
        );
    }

    #[test]
    fn file_id_missing_is_none() {
        assert_eq!(resolve_file_id("no link here"), None);
        assert_eq!(resolve_file_id("{}"), None);
    }

    #[test]
    fn csv_becomes_markdown_table() {
        let md = csv_to_markdown("Name,Qty\nBolt,12\n\"Nut, big\",5\n");
        assert!(md.contains("| Name | Qty |\n"), "md={md:?}");
        assert!(md.contains("| --- | --- |\n"));
        assert!(md.contains("| Bolt | 12 |\n"));
        assert!(md.contains("| Nut, big | 5 |\n"), "quoted comma: {md}");
    }

    #[test]
    fn missing_credentials_are_unavailable_with_guidance() {
        // Env vars are typically unset in CI; if a token IS configured on a
        // dev machine, accept it and skip the negative assertion.
        let result = resolve_access_token();
        match result {
            Err(msg) => {
                assert!(msg.contains("ASTRIA_GDRIVE_ACCESS_TOKEN"));
                assert!(msg.contains("gcloud"));
            }
            Ok(_) => {}
        }
    }

    #[test]
    fn unsupported_extension_is_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.gsheet3");
        std::fs::write(&file, "{}").unwrap();
        match export_to_markdown(&file) {
            Err(GwsError::Failed(msg)) => assert!(msg.contains("unsupported")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
