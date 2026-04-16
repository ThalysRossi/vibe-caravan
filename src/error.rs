use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CaravanError {
    #[error("cli error: {0}")]
    Cli(String),
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("not implemented: {0}")]
    NotImplemented(&'static str),
    #[error("resume ({class}): {detail}")]
    Resume { class: String, detail: String },
    #[error("io error: {0}")]
    Io(String),
    #[error("state corruption: {0}")]
    StateCorrupt(String),
    #[error("failed to read state file {path}: {source}")]
    StateRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse state file {path}: {source}")]
    StateParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("graceful shutdown requested")]
    GracefulShutdown,
}
