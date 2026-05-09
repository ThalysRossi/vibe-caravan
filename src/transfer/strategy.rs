use crate::config::{CopyStrategy, Mode};
use crate::platform::is_windows_build;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedCopyStrategy {
    Os,
    NativePreferred,
}

pub fn resolve_copy_strategy(strategy: CopyStrategy, mode: &Mode) -> ResolvedCopyStrategy {
    match strategy {
        CopyStrategy::Buffered => ResolvedCopyStrategy::Os,
        CopyStrategy::Native => {
            if is_windows_build() {
                ResolvedCopyStrategy::NativePreferred
            } else {
                ResolvedCopyStrategy::Os
            }
        }
        CopyStrategy::Auto => {
            if is_windows_build() && matches!(mode, Mode::Staging) {
                ResolvedCopyStrategy::NativePreferred
            } else {
                ResolvedCopyStrategy::Os
            }
        }
    }
}
