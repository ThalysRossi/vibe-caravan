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
}
