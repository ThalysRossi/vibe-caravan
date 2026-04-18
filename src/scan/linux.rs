use std::path::Path;

use crate::error::CaravanError;
use crate::models::file_entry::FileEntry;

use super::visit_dir_std;

pub(super) fn visit_dir_win32(
    source_root: &Path,
    start_dir: &Path,
    output: &mut Vec<FileEntry>,
) -> Result<(), CaravanError> {
    // Non-Windows fallback for tests and cross-platform behavior.
    visit_dir_std(source_root, start_dir, output)
}
