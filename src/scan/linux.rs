use std::path::Path;

use crate::error::CaravanError;
use crate::models::file_entry::FileEntry;
use crate::progress::ProgressReporter;

use super::visit_dir_std;

pub(super) fn visit_dir_win32(
    source_root: &Path,
    start_dir: &Path,
    output: &mut Vec<FileEntry>,
    progress: &mut dyn ProgressReporter,
    check_interrupt: &mut dyn FnMut() -> Result<(), CaravanError>,
) -> Result<(), CaravanError> {
    // Non-Windows fallback for tests and cross-platform behavior.
    visit_dir_std(source_root, start_dir, output, progress, check_interrupt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visit_dir_win32_uses_std_fallback_on_linux() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let source_root = tmp.path();
        let file_path = source_root.join("movies").join("movie.mkv");
        std::fs::create_dir_all(file_path.parent().expect("parent")).expect("create parent");
        std::fs::write(&file_path, b"content").expect("write file");

        let mut output = Vec::new();
        let mut progress = crate::progress::NoopProgress;
        let mut no_interrupt = || Ok(());
        visit_dir_win32(
            source_root,
            source_root,
            &mut output,
            &mut progress,
            &mut no_interrupt,
        )
        .expect("scan should succeed");

        assert_eq!(output.len(), 1);
        assert_eq!(
            output[0].relative_path,
            std::path::PathBuf::from("movies/movie.mkv")
        );
        assert_eq!(output[0].size_bytes, 7);
    }
}
