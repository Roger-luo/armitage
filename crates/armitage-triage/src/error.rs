use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error in {path}: {source}")]
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("LLM invocation failed: {0}")]
    LlmInvocation(String),

    #[error("LLM output parse error: {0}")]
    LlmParse(String),

    #[error(transparent)]
    Core(#[from] armitage_core::error::Error),

    #[error(transparent)]
    Github(#[from] armitage_github::error::Error),

    #[error(transparent)]
    Labels(#[from] armitage_labels::error::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
