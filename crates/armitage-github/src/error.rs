use armitage_core::error::CommonError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Common(#[from] CommonError),

    #[error("GitHub CLI error: {0}")]
    Cli(#[from] ionem::shell::CliError),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Common(CommonError::Io(e))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
