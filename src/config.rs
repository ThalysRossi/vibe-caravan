use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Staging,
    Migrate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationMode {
    Structural,
    Digest,
    Strict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferConfig {
    pub mode: Mode,
    pub source: PathBuf,
    pub dest: PathBuf,
    pub batch_size_bytes: u64,
    pub max_files: Option<u64>,
    pub snapshot_every: Option<u32>,
    pub interactive: bool,
    pub verification: VerificationMode,
    pub log_level: String,
    pub skip_conflicts: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Config {
    Staging(TransferConfig),
    Migrate(TransferConfig),
    Status { state: PathBuf, log_level: String },
    Resume { state: PathBuf, log_level: String },
}
