#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FileType {
    Code,
    Document,
    Paper,
    Image,
    Video,
}

impl FileType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileType::Code => "code",
            FileType::Document => "document",
            FileType::Paper => "paper",
            FileType::Image => "image",
            FileType::Video => "video",
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
            _ => None,
        }
    }
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
        ] {
            assert_eq!(FileType::from_str(ft.as_str()), Some(ft));
        }
    }
}
