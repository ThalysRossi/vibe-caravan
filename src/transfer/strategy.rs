use crate::config::{CopyStrategy, Mode};
use crate::platform::is_windows_build;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedCopyStrategy {
    Os,
    NativePreferred,
}

pub fn normalize_legacy_copy_strategy(strategy: CopyStrategy) -> CopyStrategy {
    match strategy {
        CopyStrategy::Auto => CopyStrategy::Auto,
        CopyStrategy::Buffered => {
            eprintln!(
                "[WARNING] state uses deprecated copy strategy 'buffered'; falling back to 'auto'."
            );
            CopyStrategy::Auto
        }
        CopyStrategy::Native if !is_windows_build() => {
            eprintln!(
                "[WARNING] state uses deprecated Linux copy strategy 'native'; falling back to 'auto'."
            );
            CopyStrategy::Auto
        }
        CopyStrategy::Native => CopyStrategy::Native,
    }
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
