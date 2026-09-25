// graphify-ingest: URL ingestion — fetch remote content and save it as
// graph-ready files (parity with upstream graphify v8 ingest.py):
// webpages → annotated markdown, arXiv → abstract paper notes,
// tweets → oEmbed text, images/PDFs → binary downloads.

pub mod postgres;
pub mod scip;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use graphify_core::GraphifyError;
use graphify_core::Result;

/// Maximum download size: 50 MB.
const MAX_DOWNLOAD_BYTES: usize = 50 * 1024 * 1024;
/// Timeout for all ingestion requests.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Options carried into the saved file's frontmatter.
#[derive(Debug, Clone, Default)]
pub struct IngestOptions {
    pub author: Option<String>,
    pub contributor: Option<String>,
}

/// Classified URL kind, driving the save strategy.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum UrlKind {
    Tweet,
    ArxivAbstract,
    ArxivPdf,
    Image,
    Pdf,
    Webpage,
}

pub fn classify_url(url: &str) -> UrlKind {
    let lower = url.to_lowercase();
    if lower.contains("twitter.com/") || lower.contains("x.com/") {
        return UrlKind::Tweet;
    }
    if lower.contains("arxiv.org/") {
        if lower.contains("/pdf/") || lower.ends_with(".pdf") {
            return UrlKind::ArxivPdf;
        }
        return UrlKind::ArxivAbstract;
    }
    if lower.ends_with(".pdf") {
        return UrlKind::Pdf;
    }
    for ext in [".png", ".jpg", ".jpeg", ".webp", ".gif"] {
        if lower.ends_with(ext) {
            return UrlKind::Image;
        }
    }
    UrlKind::Webpage
}

/// Fetch a URL and save graph-ready content into `out_dir`.
/// Returns the path of the saved file.
pub fn ingest_url(url: &str, out_dir: &Path, opts: &IngestOptions) -> Result<PathBuf> {
    validate_url(url)?;
    std::fs::create_dir_all(out_dir)?;

    match classify_url(url) {
        UrlKind::Tweet => save_markdown(out_dir, tweet_markdown(url, opts)?),
        UrlKind::ArxivAbstract => save_markdown(out_dir, arxiv_markdown(url, opts)),
        UrlKind::ArxivPdf | UrlKind::Pdf => save_binary(url, out_dir, pdf_name(url)),
        UrlKind::Image => save_binary(url, out_dir, image_name(url)),
        UrlKind::Webpage => {
            let (bytes, _content_type) = fetch_bytes(url)?;
            if looks_like_html(&bytes) {
                save_markdown(out_dir, webpage_markdown(url, &bytes, opts))
            } else {
                // Plain text resource
                let text = String::from_utf8_lossy(&bytes).to_string();
                save_markdown(
                    out_dir,
                    annotated_markdown(url, "text", &text, &derive_title(&text), opts),
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Kind-specific handlers
// ---------------------------------------------------------------------------

/// Tweets via the publish.twitter.com oEmbed endpoint, degraded to a stub
/// when the service is unreachable.
fn tweet_markdown(url: &str, opts: &IngestOptions) -> Result<String> {
    let canonical = url.replace("x.com/", "twitter.com/");
    let oembed = format!(
        "https://publish.twitter.com/oembed?url={}",
        urlencode(&canonical)
    );
    let (body, _) = fetch_bytes(&oembed)?;
    let json: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| GraphifyError::Graph(format!("oEmbed response not JSON: {e}")))?;
    let html = json.get("html").and_then(|h| h.as_str()).unwrap_or("");
    let author = json
        .get("author_name")
        .and_then(|a| a.as_str())
        .unwrap_or("unknown");
    let text = strip_tags(html);
    Ok(annotated_markdown(
        url,
        "tweet",
        &format!("Tweet by @{author}\n\n{text}\n\nSource: {url}\n"),
        &format!("tweet-{author}"),
        opts,
    ))
}

/// arXiv abstract page → title/authors/abstract paper note.
fn arxiv_markdown(url: &str, opts: &IngestOptions) -> String {
    let id = arxiv_id(url).unwrap_or_else(|| "unknown".into());
    let abs_url = format!("https://arxiv.org/abs/{id}");
    let page = fetch_bytes(&abs_url).map(|(b, _)| b).unwrap_or_default();
    let html = String::from_utf8_lossy(&page).to_string();

    let title = extract_meta(&html, "citation_title").unwrap_or_else(|| id.clone());
    let authors: Vec<String> = extract_meta_all(&html, "citation_author");
    let abstract_text = extract_meta(&html, "citation_abstract")
        .or_else(|| extract_meta(&html, "og:description"))
        .unwrap_or_default();

    annotated_markdown(
        url,
        "paper",
        &format!(
            "# {title}\n\n**Authors:** {}\n**arXiv:** {id}\n\n## Abstract\n\n{abstract_text}\n\nSource: {url}\n",
            authors.join(", "),
        ),
        &title,
        opts,
    )
}

fn webpage_markdown(url: &str, bytes: &[u8], opts: &IngestOptions) -> String {
    let html = String::from_utf8_lossy(bytes).to_string();
    let title = extract_tag(&html, "title").unwrap_or_else(|| url.to_string());
    let text = html_to_text(bytes);
    annotated_markdown(url, "webpage", &text, &title, opts)
}

/// Build the annotated markdown document with YAML frontmatter.
fn annotated_markdown(
    url: &str,
    kind: &str,
    body: &str,
    title: &str,
    opts: &IngestOptions,
) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let safe_title = title.replace('"', "'");
    let mut front = format!(
        "---\nsource_url: {url}\ntype: {kind}\ntitle: \"{safe_title}\"\ncaptured_at: {now}\n"
    );
    if let Some(a) = &opts.author {
        front.push_str(&format!("author: \"{}\"\n", a.replace('"', "'")));
    }
    if let Some(c) = &opts.contributor {
        front.push_str(&format!("contributor: \"{}\"\n", c.replace('"', "'")));
    }
    front.push_str("---\n\n");
    format!("{front}{body}")
}

// ---------------------------------------------------------------------------
// Fetch + save primitives
// ---------------------------------------------------------------------------

fn fetch_bytes(url: &str) -> Result<(Vec<u8>, String)> {
    validate_url(url)?;
    let agent = ureq::config::Config::builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .new_agent();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| GraphifyError::Graph(format!("Failed to fetch {url}: {e}")))?;
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mut reader = response.into_body().into_reader();
    let mut bytes = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| GraphifyError::Graph(format!("Download read error: {e}")))?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        if bytes.len() > MAX_DOWNLOAD_BYTES {
            return Err(GraphifyError::Graph(format!(
                "Download exceeded {} byte limit for {url}",
                MAX_DOWNLOAD_BYTES
            )));
        }
    }
    Ok((bytes, content_type))
}

fn save_markdown(out_dir: &Path, content: String) -> Result<PathBuf> {
    // First heading or source line gives a stable-ish name; fall back to timestamp.
    let name = derive_doc_name(&content);
    unique_write(out_dir, &name, content.as_bytes())
}

fn save_binary(url: &str, out_dir: &Path, name: String) -> Result<PathBuf> {
    let (bytes, _) = fetch_bytes(url)?;
    unique_write(out_dir, &name, &bytes)
}

/// Write with `_1`, `_2`… suffixing on collision.
fn unique_write(out_dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf> {
    let mut path = out_dir.join(name);
    let mut counter = 1;
    while path.exists() {
        let stem = Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("doc");
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("md");
        path = out_dir.join(format!("{stem}_{counter}.{ext}"));
        counter += 1;
    }
    std::fs::write(&path, bytes)?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Naming helpers
// ---------------------------------------------------------------------------

fn derive_doc_name(content: &str) -> String {
    // Prefer the first `# ` heading, else the source_url frontmatter line.
    let heading = content
        .lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches("# ").trim().to_string());
    let source = content
        .lines()
        .find(|l| l.starts_with("source_url: "))
        .map(|l| l.trim_start_matches("source_url: ").trim().to_string());
    let raw = heading.or(source).unwrap_or_else(|| "document".into());
    let slug: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let slug = slug.trim_matches('_').to_string();
    let mut name = if slug.is_empty() {
        "document".to_string()
    } else {
        slug
    };
    name.truncate(80);
    format!("{name}.md")
}

fn pdf_name(url: &str) -> String {
    match arxiv_id(url) {
        Some(id) => format!("arxiv_{}.pdf", id.replace('.', "_")),
        None => format!("{}.pdf", url.rsplit('/').next().unwrap_or("document")),
    }
}

fn image_name(url: &str) -> String {
    let seg = url.rsplit('/').next().unwrap_or("image");
    let seg = seg.split('?').next().unwrap_or("image");
    if seg.contains('.') {
        seg.to_string()
    } else {
        "image.png".to_string()
    }
}

fn arxiv_id(url: &str) -> Option<String> {
    // Pattern: \d{4}\.\d{4,5} (e.g. 1706.03762, optionally with a vN suffix)
    let b = url.as_bytes();
    for i in 0..b.len() {
        if i + 5 < b.len()
            && b[i..i + 4].iter().all(u8::is_ascii_digit)
            && b[i + 4] == b'.'
            && b[i + 5].is_ascii_digit()
        {
            let mut j = i + 5;
            while j < b.len() && b[j].is_ascii_digit() && j - i <= 10 {
                j += 1;
            }
            return Some(url[i..j].to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// HTML helpers
// ---------------------------------------------------------------------------

/// Extract a `<meta name="..." content="...">` value (citation_* tags).
fn extract_meta(html: &str, name: &str) -> Option<String> {
    let needle = format!("name=\"{name}\"");
    let pos = html.find(&needle)?;
    let rest = &html[pos + needle.len()..];
    let content_pos = rest.find("content=\"")?;
    let after = &rest[content_pos + "content=\"".len()..];
    let end = after.find('"')?;
    Some(decode_entities(&after[..end]))
}

/// All values for a repeated meta tag (citation_author).
fn extract_meta_all(html: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let needle = format!("name=\"{name}\"");
    let mut search_from = 0;
    while let Some(pos) = html[search_from..].find(&needle) {
        let tag_start = search_from + pos;
        // Reuse extract_meta on a window starting at this tag
        if let Some(v) = extract_meta(&html[tag_start..], name) {
            out.push(v);
        }
        search_from = tag_start + needle.len();
    }
    out
}

fn extract_tag(html: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = html.find(&open)?;
    let after_open = &html[start..];
    let content_start = after_open.find('>')? + 1;
    let close = format!("</{tag}>");
    let end = after_open.find(&close)?;
    Some(
        strip_tags(&after_open[content_start..end])
            .trim()
            .to_string(),
    )
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
}

fn looks_like_html(bytes: &[u8]) -> bool {
    let head = if bytes.len() > 512 {
        &bytes[..512]
    } else {
        bytes
    };
    let s = String::from_utf8_lossy(head).to_lowercase();
    s.contains("<html") || s.contains("<!doctype html") || s.contains("<head")
}

fn html_to_text(bytes: &[u8]) -> String {
    strip_tags(&String::from_utf8_lossy(bytes))
}

fn derive_title(text: &str) -> String {
    text.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("document")
        .chars()
        .take(80)
        .collect()
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// URL validation (SSRF guards)
// ---------------------------------------------------------------------------

fn validate_url(url: &str) -> Result<()> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(GraphifyError::Graph(format!(
            "Unsupported URL scheme (only http/https allowed): {url}"
        )));
    }
    if let Some(host) = extract_host(url) {
        if is_private_host(&host) {
            return Err(GraphifyError::Graph(format!(
                "URL resolves to a private/internal address (blocked): {url}"
            )));
        }
    }
    Ok(())
}

fn extract_host(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let host = rest.split('/').next()?;
    let host = host.split(':').next()?; // strip port
    let host = host.rsplit('@').next()?; // strip userinfo
    Some(host.to_lowercase())
}

fn is_private_host(host: &str) -> bool {
    matches!(
        host,
        "localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]" | "::1" | "metadata.google.internal"
    ) || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || host.starts_with("169.254.")
        || host.starts_with("172.16.")
        || host.starts_with("172.17.")
        || host.starts_with("172.18.")
        || host.starts_with("172.19.")
        || host.starts_with("172.2")
        || host.starts_with("172.30.")
        || host.starts_with("172.31.")
        || host.ends_with(".internal")
        || host.ends_with(".local")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_urls() {
        assert_eq!(
            classify_url("https://x.com/karpathy/status/123"),
            UrlKind::Tweet
        );
        assert_eq!(
            classify_url("https://twitter.com/a/status/1"),
            UrlKind::Tweet
        );
        assert_eq!(
            classify_url("https://arxiv.org/abs/1706.03762"),
            UrlKind::ArxivAbstract
        );
        assert_eq!(
            classify_url("https://arxiv.org/pdf/1706.03762"),
            UrlKind::ArxivPdf
        );
        assert_eq!(classify_url("https://example.com/paper.pdf"), UrlKind::Pdf);
        assert_eq!(
            classify_url("https://example.com/diagram.PNG"),
            UrlKind::Image
        );
        assert_eq!(classify_url("https://example.com/page"), UrlKind::Webpage);
    }

    #[test]
    fn blocks_non_http_schemes() {
        let err = validate_url("file:///etc/passwd").unwrap_err();
        assert!(err.to_string().contains("scheme"));
    }

    #[test]
    fn blocks_private_hosts() {
        for url in [
            "http://localhost/admin",
            "http://127.0.0.1/x",
            "http://192.168.1.1/router",
            "http://169.254.169.254/metadata",
            "http://foo.internal/x",
        ] {
            assert!(validate_url(url).is_err(), "should block {url}");
        }
        assert!(validate_url("https://example.com/page").is_ok());
    }

    #[test]
    fn frontmatter_annotates_documents() {
        let md = annotated_markdown(
            "https://example.com/a",
            "webpage",
            "body text",
            "My \"Page\"",
            &IngestOptions {
                author: Some("Alice".into()),
                contributor: Some("Bob".into()),
            },
        );
        assert!(md.starts_with("---\n"));
        assert!(md.contains("source_url: https://example.com/a"));
        assert!(md.contains("type: webpage"));
        assert!(md.contains("title: \"My 'Page'\""));
        assert!(md.contains("author: \"Alice\""));
        assert!(md.contains("contributor: \"Bob\""));
        assert!(md.contains("body text"));
    }

    #[test]
    fn collision_suffixing() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = unique_write(dir.path(), "doc.md", b"first").unwrap();
        let p2 = unique_write(dir.path(), "doc.md", b"second").unwrap();
        assert_eq!(p1.file_name().unwrap(), "doc.md");
        assert_eq!(p2.file_name().unwrap(), "doc_1.md");
        assert_eq!(std::fs::read_to_string(&p2).unwrap(), "second");
    }

    #[test]
    fn doc_names_derived_from_heading() {
        let md = annotated_markdown(
            "https://x.com/a/status/1",
            "tweet",
            "# Hello World\n\ntext",
            "t",
            &IngestOptions::default(),
        );
        assert_eq!(derive_doc_name(&md), "hello_world.md");
    }

    #[test]
    fn meta_extraction() {
        let html = r#"<meta name="citation_title" content="Attention Is All &amp; You Need">"#;
        assert_eq!(
            extract_meta(html, "citation_title").unwrap(),
            "Attention Is All & You Need"
        );
        let html2 = "<title>My Page</title><body>x</body>";
        assert_eq!(extract_tag(html2, "title").unwrap(), "My Page");
    }

    #[test]
    fn arxiv_id_extraction() {
        assert_eq!(
            arxiv_id("https://arxiv.org/abs/1706.03762v2"),
            Some("1706.03762".into())
        );
        assert_eq!(
            arxiv_id("https://arxiv.org/pdf/2401.12345"),
            Some("2401.12345".into())
        );
    }

    #[test]
    fn urlencode_handles_reserved() {
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
    }

    #[test]
    fn strip_tags_collapses() {
        assert_eq!(strip_tags("<p>hello <b>world</b></p>"), "hello world");
    }
}
