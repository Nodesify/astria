#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FileType {
    Code,
    Document,
    Paper,
    Image,
    Video,
    Audio,
}

impl FileType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileType::Code => "code",
            FileType::Document => "document",
            FileType::Paper => "paper",
            FileType::Image => "image",
            FileType::Video => "video",
            FileType::Audio => "audio",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "code" => Some(FileType::Code),
            "document" => Some(FileType::Document),
            "paper" => Some(FileType::Paper),
            "image" => Some(FileType::Image),
            "video" => Some(FileType::Video),
            "audio" => Some(FileType::Audio),
            _ => None,
        }
    }
}

/// Media extensions transcribed to text via the external `whisper-cli`
/// binary (whisper.cpp). Video additionally needs `ffmpeg` to demux the
/// audio track. Shared by detect (file classification) and extract
/// (transcription routing) — keep in sync with astria-audio's handling.
pub const VIDEO_EXTENSIONS: &[&str] = &[".mp4", ".mov", ".webm", ".mkv", ".avi"];
pub const AUDIO_EXTENSIONS: &[&str] = &[
    ".mp3", ".wav", ".m4a", ".flac", ".ogg", ".opus", ".aac", ".wma",
];

/// True when `ext` (with or without a leading dot, any case) is a media file
/// the transcription route in astria-extract handles.
pub fn is_transcribable_extension(ext: &str) -> bool {
    let trimmed = ext.trim_start_matches('.');
    let with_dot = format!(".{trimmed}").to_lowercase();
    VIDEO_EXTENSIONS.contains(&with_dot.as_str()) || AUDIO_EXTENSIONS.contains(&with_dot.as_str())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub community_count: usize,
    pub file_count: usize,
}

/// Dependency manifests ingested deterministically (never via LLM).
/// Shared by detect (file classification) and extract (manifest parsing).
pub const MANIFEST_FILENAMES: &[&str] = &[
    "pyproject.toml",
    "cargo.toml",
    "go.mod",
    "package.json",
    "pom.xml",
    // MCP server configs: agent toolchains are part of the graph too.
    ".mcp.json",
    "mcp.json",
    "mcp_servers.json",
    "claude_desktop_config.json",
];

/// True when `file_name` (any case) is an MCP server configuration — files
/// that routinely embed literal credentials (`env` blocks, API keys). They
/// are ingested by the deterministic manifest extractor, which preserves
/// only tool names and environment variable *names*; the raw bytes must
/// never reach an LLM backend or any other raw-content consumer.
pub fn is_mcp_config_filename(file_name: &str) -> bool {
    matches!(
        file_name.to_lowercase().as_str(),
        ".mcp.json" | "mcp.json" | "mcp_servers.json" | "claude_desktop_config.json"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_type_roundtrip() {
        for ft in [
            FileType::Code,
            FileType::Document,
            FileType::Paper,
            FileType::Image,
            FileType::Video,
            FileType::Audio,
        ] {
            assert_eq!(FileType::from_str(ft.as_str()), Some(ft));
        }
    }

    #[test]
    fn transcribable_extension_routing() {
        assert!(is_transcribable_extension("mp4"));
        assert!(is_transcribable_extension("MOV"));
        assert!(is_transcribable_extension("mp3"));
        assert!(is_transcribable_extension("wav"));
        assert!(!is_transcribable_extension("png"));
        assert!(!is_transcribable_extension(""));
        // Every listed extension round-trips through the classifier.
        for ext in VIDEO_EXTENSIONS.iter().chain(AUDIO_EXTENSIONS.iter()) {
            assert!(
                is_transcribable_extension(&ext[1..]),
                "listed extension {ext} must route"
            );
        }
    }
}
