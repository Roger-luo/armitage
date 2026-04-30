#[derive(Debug, thiserror::Error)]
pub enum Error {
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
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    #[error("GitHub CLI error: {0}")]
    Cli(#[from] ionem::shell::CliError),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
