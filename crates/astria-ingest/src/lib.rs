// astria-ingest: URL ingestion — fetch remote content and save it as
// graph-ready files (parity with upstream astria v8 ingest.py):
// webpages → annotated markdown, arXiv → abstract paper notes,
// tweets → oEmbed text, images/PDFs → binary downloads.

pub mod postgres;
pub mod scip;

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use astria_core::AstriaError;
use astria_core::Result;
use url::Url;

/// Maximum download size: 50 MB.
const MAX_DOWNLOAD_BYTES: usize = 50 * 1024 * 1024;
/// Timeout for all ingestion requests.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Maximum redirect hops followed manually; every hop is re-validated.
const MAX_REDIRECTS: usize = 5;

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
        .map_err(|e| AstriaError::Graph(format!("oEmbed response not JSON: {e}")))?;
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
    // Auto-follow is disabled so redirects surface here and every hop
    // re-runs URL validation: a public server answering 302 -> internal
    // address must not become an internal fetch.
    let agent = ureq::config::Config::builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .max_redirects(0)
        .build()
        .new_agent();
    let mut current = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        validate_url(&current)?;
        validate_resolved_hosts(&current)?;
        let response = agent
            .get(&current)
            .call()
            .map_err(|e| AstriaError::Graph(format!("Failed to fetch {current}: {e}")))?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    AstriaError::Graph(format!("redirect from {current} without a Location header"))
                })?;
            current = resolve_redirect(&current, location)?;
            continue;
        }
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
                .map_err(|e| AstriaError::Graph(format!("Download read error: {e}")))?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buf[..n]);
            if bytes.len() > MAX_DOWNLOAD_BYTES {
                return Err(AstriaError::Graph(format!(
                    "Download exceeded {} byte limit for {url}",
                    MAX_DOWNLOAD_BYTES
                )));
            }
        }
        return Ok((bytes, content_type));
    }
    Err(AstriaError::Graph(format!(
        "exceeded {MAX_REDIRECTS} redirect hops starting at {url}"
    )))
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
        None => {
            let seg = last_path_segment(url);
            let seg = seg.as_deref().unwrap_or("document");
            match seg.rsplit_once('.') {
                // Segment already names a pdf: keep one extension.
                Some((stem, ext)) if ext.eq_ignore_ascii_case("pdf") => {
                    format!("{}.pdf", safe_file_stem(stem, "document"))
                }
                _ => format!("{}.pdf", safe_file_stem(seg, "document")),
            }
        }
    }
}

fn image_name(url: &str) -> String {
    let seg = last_path_segment(url);
    let seg = seg.as_deref().unwrap_or("image");
    match seg.rsplit_once('.') {
        Some((stem, ext))
            if !ext.is_empty()
                && ext.len() <= 5
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            format!("{}.{}", safe_file_stem(stem, "image"), ext.to_lowercase())
        }
        _ => "image.png".to_string(),
    }
}

/// Last `/`-separated path component of a URL, without query or fragment.
fn last_path_segment(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let seg = path.rsplit('/').next()?;
    Some(seg.to_string())
}

/// Slug a URL segment into a safe filename component: only ASCII
/// alphanumerics (and `_`) survive, so separators, drive letters, and
/// Windows-reserved device names cannot appear — the result can never
/// escape the output directory.
fn safe_file_stem(seg: &str, fallback: &str) -> String {
    let slug: String = seg
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let slug = slug.trim_matches(|c| c == '_' || c == '.').to_string();
    if slug.is_empty() {
        return fallback.to_string();
    }
    if is_windows_reserved(&slug) {
        format!("f_{slug}")
    } else {
        slug
    }
}

/// Windows treats these (any case) as devices regardless of extension.
fn is_windows_reserved(stem: &str) -> bool {
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
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
        return Err(AstriaError::Graph(format!(
            "Unsupported URL scheme (only http/https allowed): {url}"
        )));
    }
    if let Some(host) = extract_host(url) {
        if is_private_host(&host) {
            return Err(AstriaError::Graph(format!(
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

/// Join a redirect `Location` value against the request URL it came from.
/// Absolute, `//host/path`, `/path`, and bare-relative forms all resolve via
/// the url crate; the loop's `validate_url` call then rejects any target
/// whose scheme is not http/https (e.g. `file://`).
fn resolve_redirect(base: &str, location: &str) -> Result<String> {
    let joined = Url::parse(base)
        .and_then(|b| b.join(location))
        .map_err(|e| AstriaError::Graph(format!("bad redirect target '{location}': {e}")))?;
    Ok(joined.to_string())
}

/// DNS-resolve `url`'s host and reject any address that is not public.
/// Catches what the string-level `validate_url` checks cannot see: DNS names
/// that resolve to internal addresses (rebinding or lookalike records) and
/// non-canonical IPv4 spellings (`2130706433`, `0x7f000001`), which the OS
/// resolver normalizes to an `IpAddr` before we inspect it. Called before
/// every request, including every manual redirect hop.
///
/// Residual risk: ureq re-resolves independently at connect time, so a
/// rebinding attacker with a fast-TTL record can still win the race between
/// this check and the connection. Blocking is standard-practice mitigation,
/// not a hard guarantee.
fn validate_resolved_hosts(url: &str) -> Result<()> {
    let parsed =
        Url::parse(url).map_err(|e| AstriaError::Graph(format!("invalid URL {url}: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| AstriaError::Graph(format!("URL has no host: {url}")))?;
    if is_private_host(host) {
        return Err(AstriaError::Graph(format!(
            "URL resolves to a private/internal address (blocked): {url}"
        )));
    }
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs: Vec<IpAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| AstriaError::Graph(format!("failed to resolve {host}: {e}")))?
        .map(|a| a.ip())
        .collect();
    if addrs.is_empty() {
        return Err(AstriaError::Graph(format!(
            "host resolved to no addresses: {host}"
        )));
    }
    for ip in addrs {
        if is_private_ip(ip) {
            return Err(AstriaError::Graph(format!(
                "URL resolves to a private/internal address (blocked): {url}"
            )));
        }
    }
    Ok(())
}

/// True for any address a user/agent-supplied URL must not be fetched from:
/// loopback, private ranges, link-local (incl. cloud metadata
/// 169.254.169.254), CGNAT, benchmarking, multicast/reserved/broadcast, and
/// the IPv6 equivalents including IPv4-mapped (`::ffff:127.0.0.1`) and
/// IPv4-compatible (`::127.0.0.1`) forms.
fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4() {
            // Mapped and IPv4-compatible forms carry the embedded v4's risk.
            Some(v4) => is_private_ipv4(v4),
            None => {
                // 2001:db8::/32 (documentation) has no stable std check.
                let s = v6.segments();
                v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_unique_local()
                    || v6.is_unicast_link_local()
                    || v6.is_multicast()
                    || (s[0] == 0x2001 && s[1] == 0x0db8)
            }
        },
    }
}

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_loopback()                                 // 127.0.0.0/8
        || ip.is_private()                           // 10/8, 172.16/12, 192.168/16
        || ip.is_link_local()                        // 169.254/16 (cloud metadata)
        || ip.is_unspecified()                       // 0.0.0.0
        || o[0] == 0                                 // 0.0.0.0/8 "this network"
        || o[0] == 100 && (64..=127).contains(&o[1]) // 100.64.0.0/10 CGNAT
        || (o[0], o[1], o[2]) == (192, 0, 0)         // 192.0.0.0/24 IETF assignments
        || (o[0], o[1], o[2]) == (192, 0, 2)         // 192.0.2.0/24 TEST-NET-1
        || o[0] == 198 && (18..=19).contains(&o[1])  // 198.18.0.0/15 benchmarking
        || o[0] >= 224 // multicast 224/4, reserved 240/4
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

    // -- SSRF: resolved-address checks --

    #[test]
    fn private_ipv4_ranges_blocked() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.20.5.5",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "0.1.2.3",
            "100.64.0.1",
            "100.127.255.255",
            "198.18.0.1",
            "198.19.255.255",
            "192.0.0.1",
            "192.0.2.9",
            "224.0.0.1",
            "255.255.255.255",
        ] {
            assert!(
                is_private_ip(ip.parse().unwrap()),
                "{ip} must be treated as private"
            );
        }
        for ip in [
            "8.8.8.8",
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.1",
            "172.32.0.1",
            "198.20.0.1",
            "203.0.113.99",
        ] {
            assert!(
                !is_private_ip(ip.parse().unwrap()),
                "{ip} must be treated as public"
            );
        }
    }

    #[test]
    fn private_ipv6_ranges_blocked() {
        for ip in [
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12:3456::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "::127.0.0.1",
        ] {
            assert!(
                is_private_ip(ip.parse().unwrap()),
                "{ip} must be treated as private"
            );
        }
        assert!(!is_private_ip("2606:4700::1111".parse().unwrap()));
    }

    #[test]
    fn resolved_host_check_blocks_loopback() {
        // `localhost` and IP literals resolve without any network access, so
        // this test is hermetic.
        assert!(validate_resolved_hosts("http://localhost/x").is_err());
        assert!(validate_resolved_hosts("https://127.0.0.1/x").is_err());
        assert!(validate_resolved_hosts("http://[::1]:8080/x").is_err());
    }

    // -- SSRF: redirect targets --

    #[test]
    fn resolve_redirect_joins_all_location_forms() {
        assert_eq!(
            resolve_redirect("https://a.com/x/y", "/z").unwrap(),
            "https://a.com/z"
        );
        assert_eq!(
            resolve_redirect("https://a.com/x/y", "other").unwrap(),
            "https://a.com/x/other"
        );
        assert_eq!(
            resolve_redirect("https://a.com/x", "//b.com/p?q=1").unwrap(),
            "https://b.com/p?q=1"
        );
        assert_eq!(
            resolve_redirect("https://a.com/", "https://b.com/p").unwrap(),
            "https://b.com/p"
        );
    }

    #[test]
    fn resolve_redirect_rejects_garbage() {
        assert!(resolve_redirect("https://a.com/", "http://").is_err());
        assert!(resolve_redirect("not a url at all", "/x").is_err());
    }

    #[test]
    fn redirect_scheme_downgrade_blocked_upstream_of_join() {
        // The loop re-runs validate_url on the joined target, so a `file://`
        // Location resolves to an absolute URL here but is rejected there.
        assert_eq!(
            resolve_redirect("https://a.com/", "file:///etc/passwd").unwrap(),
            "file:///etc/passwd"
        );
        assert!(validate_url("file:///etc/passwd").is_err());
    }

    // -- Filename safety --

    #[test]
    fn download_names_cannot_traverse() {
        // Windows path separators in a URL segment must not become separators
        // in the saved filename.
        assert_eq!(image_name(r"https://evil.com/..\..\evil.png"), "evil.png");
        assert_eq!(pdf_name(r"https://evil.com/p/..\..\evil"), "evil.pdf");
        assert!(!image_name(r"https://evil.com/..\..\evil.png").contains(['\\', '/']));
    }

    #[test]
    fn download_names_strip_query_and_normalize() {
        assert_eq!(image_name("https://x.com/img/pic.JPEG?w=100"), "pic.jpeg");
        assert_eq!(pdf_name("https://x.com/a/report.pdf?dl=1"), "report.pdf");
        assert_eq!(image_name("https://x.com/a/b/"), "image.png");
        assert_eq!(image_name("https://x.com/diagram"), "image.png");
        // Windows-reserved device names are defused.
        assert_eq!(image_name("https://x.com/NUL.png"), "f_nul.png");
    }

    #[test]
    fn strip_tags_collapses() {
        assert_eq!(strip_tags("<p>hello <b>world</b></p>"), "hello world");
    }
}
