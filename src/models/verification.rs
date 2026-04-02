use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DigestModeUsed {
    None,
    Blake3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReport {
    pub file_count: usize,
    pub bytes_compared: u64,
    pub missing_files: Vec<String>,
    pub mismatched_files: Vec<String>,
    pub unreadable_files: Vec<String>,
    pub digest_mode_used: DigestModeUsed,
    pub status: VerificationStatus,
    pub recommended_action: String,
}
