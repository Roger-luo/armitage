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

    #[error("not an org directory (no armitage.toml found)")]
    NotInOrg,

    #[error("node not found: {0}")]
    NodeNotFound(String),

    #[error("parent node not found: {0}")]
    ParentNotFound(String),

    #[error("node already exists: {0}")]
    NodeExists(String),

    #[error("invalid issue reference: {0} (expected owner/repo#number)")]
    InvalidIssueRef(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
