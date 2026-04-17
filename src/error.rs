use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationFailure {
    pub recommended_action: String,
    pub missing_files: Vec<String>,
    pub mismatched_files: Vec<String>,
    pub unreadable_files: Vec<String>,
}

impl VerificationFailure {
    pub fn from_report(report: crate::models::verification::VerificationReport) -> Self {
        Self {
            recommended_action: report.recommended_action,
            missing_files: report.missing_files,
            mismatched_files: report.mismatched_files,
            unreadable_files: report.unreadable_files,
        }
    }
}

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
    #[error("{context}: {source}")]
    IoContext {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("policy blocked: {0}")]
    PolicyBlocked(String),
    #[error("verification failed")]
    VerificationFailed(VerificationFailure),
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
