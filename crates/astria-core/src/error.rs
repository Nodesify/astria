#[derive(Debug, thiserror::Error)]
pub enum AstriaError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Parse error in {file}: {message}")]
    Parse { file: String, message: String },

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("semantic extraction incomplete: processed {processed_chunks}/{total_chunks} chunks; {reason}")]
    SemanticIncomplete {
        processed_chunks: usize,
        total_chunks: usize,
        reason: String,
    },

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AstriaError>;
