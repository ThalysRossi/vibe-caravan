use crate::models::file_entry::FileEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub id: String,
    pub files: Vec<FileEntry>,
    pub total_bytes: u64,
    pub file_count: usize,
}
