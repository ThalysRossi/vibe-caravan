mod atomic_copy;
mod backend;
mod batch_copy;
mod copier;
mod strategy;
mod summary;

#[cfg(target_os = "linux")]
mod platform_linux;
#[cfg(target_os = "windows")]
mod platform_windows;

pub use backend::LocalFsCopyBackend;
pub use batch_copy::{
    DirectoryCreator, FsDirectoryCreator, copy_batch_with_components_and_durability,
};
pub use copier::{FileCopier, NativePreferredFileCopier, OsFileCopier};
pub use strategy::{ResolvedCopyStrategy, resolve_copy_strategy};
pub use summary::{
    TransferExecutionSummary, TransferPlanningSummary, summarize_transfer_execution,
    summarize_transfer_plan,
};
