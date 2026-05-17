use armitage_core::error::CommonError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Common(#[from] CommonError),

    #[error("JSON serialize error: {0}")]
    JsonSerialize(#[from] serde_json::Error),

    #[error("template render error: {0}")]
    Template(#[from] askama::Error),

    #[error(transparent)]
    Core(#[from] armitage_core::error::Error),

    #[error("{0}")]
    Other(String),
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
