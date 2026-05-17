use std::path::PathBuf;

/// Common base errors shared across every crate in the workspace.
///
/// These are the I/O and TOML errors that every crate would otherwise have to
/// redeclare. Each crate's own `Error` enum embeds this via a
/// `Common(#[from] armitage_core::error::CommonError)` variant plus pass-through
/// `From<std::io::Error>` and `From<toml::ser::Error>` impls so that `?` works
/// transparently at call sites.
#[derive(Debug, thiserror::Error)]
pub enum CommonError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error in {path}: {source}")]
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Common(#[from] CommonError),

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

impl Error {
    /// Construct a `TomlParse` error with the given path and source.
    pub fn toml_parse(path: PathBuf, source: toml::de::Error) -> Self {
        Self::Common(CommonError::TomlParse { path, source })
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Common(CommonError::Io(e))
    }
}

impl From<toml::ser::Error> for Error {
    fn from(e: toml::ser::Error) -> Self {
        Self::Common(CommonError::TomlSerialize(e))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
