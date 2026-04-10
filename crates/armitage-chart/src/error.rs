#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialize error: {0}")]
    JsonSerialize(#[from] serde_json::Error),

    #[error("template render error: {0}")]
    Template(#[from] askama::Error),

    #[error(transparent)]
    Core(#[from] armitage_core::error::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
