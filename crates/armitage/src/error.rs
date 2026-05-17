use armitage_core::error::CommonError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Common(#[from] CommonError),
    #[error(transparent)]
    Core(#[from] armitage_core::error::Error),
    #[error(transparent)]
    Labels(#[from] armitage_labels::error::Error),
    #[error(transparent)]
    Github(#[from] armitage_github::error::Error),
    #[error(transparent)]
    Sync(#[from] armitage_sync::error::Error),
    #[error(transparent)]
    Triage(#[from] armitage_triage::error::Error),
    #[error(transparent)]
    Chart(#[from] armitage_chart::error::Error),
    #[error(transparent)]
    Project(#[from] armitage_project::error::Error),
    #[error("GitHub CLI error: {0}")]
    Cli(#[from] ionem::shell::CliError),
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

impl Error {
    /// Convenience for ad-hoc string errors: `Error::other(format!(...))`
    /// or `Error::other("literal")`.
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}
