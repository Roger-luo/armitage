use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("GitHub CLI error: {0}")]
    Cli(#[from] ionem::shell::CliError),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parse error in {path}: {source}")]
    TomlParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("armitage-core error: {0}")]
    Core(#[from] armitage_core::error::Error),

    #[error("armitage-github error: {0}")]
    Github(#[from] armitage_github::error::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
