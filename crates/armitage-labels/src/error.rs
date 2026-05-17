use std::path::PathBuf;

use armitage_core::error::CommonError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Common(#[from] CommonError),

    #[error(transparent)]
    Core(#[from] armitage_core::error::Error),

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
