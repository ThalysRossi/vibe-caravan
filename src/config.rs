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
    
    /// Buffer size for file copying (in bytes).
    /// Default: 8 MiB (8 * 1024 * 1024)
    #[serde(default = "TransferConfig::default_copy_buffer_size")]
    pub copy_buffer_size: usize,
    
    /// File size threshold (in bytes) to use buffered copy instead of OS copy.
    /// Files smaller than this threshold use OS copy, larger files use buffered copy.
    /// Default: 1 MiB (1 * 1024 * 1024)
    #[serde(default = "TransferConfig::default_buffered_copy_threshold")]
    pub buffered_copy_threshold: u64,
}

impl TransferConfig {
    /// Default buffer size for file copying (8 MiB)
    pub const fn default_copy_buffer_size() -> usize {
        8 * 1024 * 1024
    }
    
    /// Default threshold for using buffered copy (1 MiB)
    pub const fn default_buffered_copy_threshold() -> u64 {
        1 * 1024 * 1024
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Config {
    Staging(TransferConfig),
    Migrate(TransferConfig),
    Status { state: PathBuf, log_level: String },
    Resume { state: PathBuf, log_level: String },
}
