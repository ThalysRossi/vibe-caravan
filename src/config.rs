use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Staging,
    Migrate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CopyStrategy {
    #[default]
    Auto,
    Native,
    /// Legacy state compatibility variant; treated the same as `Auto`.
    Buffered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ConflictPolicy {
    #[default]
    SkipFile,
    SkipBatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferConfig {
    pub mode: Mode,
    pub source: PathBuf,
    pub dest: PathBuf,
    pub batch_size_bytes: u64,
    pub max_files: Option<u64>,
    pub snapshot_every: Option<u32>,
    pub snapshot_dir: Option<PathBuf>,
    pub interactive: bool,
    pub log_level: String,
    pub skip_conflicts: bool,
    #[serde(default)]
    pub conflict_policy: ConflictPolicy,
    #[serde(default)]
    pub recover_failed: bool,
    #[serde(default)]
    pub allow_unsafe_filesystems: bool,
    #[serde(default)]
    pub copy_strategy: CopyStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Config {
    Staging(TransferConfig),
    Migrate(TransferConfig),
    Status {
        state: PathBuf,
        log_level: String,
        output: OutputFormat,
    },
    Resume {
        state: PathBuf,
        log_level: String,
        recover_failed: bool,
        inspect_failed: bool,
        output: OutputFormat,
    },
}
