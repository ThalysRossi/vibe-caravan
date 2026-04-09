//! Signal handling for graceful shutdown.
//!
//! Provides cross-platform signal handling for graceful shutdown on both Windows and Linux.
//! Uses the `ctrlc` crate internally for cross-platform compatibility.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::CaravanError;

/// Global shutdown flag shared across the application.
/// When set to `true`, the application should gracefully shut down.
#[derive(Debug, Clone)]
pub struct ShutdownFlag {
    flag: Arc<AtomicBool>,
}

impl ShutdownFlag {
    /// Create a new shutdown flag initialized to `false`.
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if shutdown has been requested.
    ///
    /// Returns `true` if a graceful shutdown has been requested (e.g., via Ctrl+C).
    pub fn is_shutdown_requested(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Request shutdown by setting the flag to `true`.
    pub fn request_shutdown(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Reset the shutdown flag to `false`.
    pub fn reset(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }

    /// Get a reference to the underlying atomic flag.
    pub fn inner(&self) -> &Arc<AtomicBool> {
        &self.flag
    }
}

impl Default for ShutdownFlag {
    fn default() -> Self {
        Self::new()
    }
}

/// Install signal handlers for graceful shutdown.
///
/// This function sets up handlers for SIGINT (Ctrl+C) and SIGTERM on Unix-like systems,
/// and Ctrl+C on Windows. When a signal is received, the shutdown flag will be set.
///
/// # Errors
///
/// Returns `CaravanError::Io` if signal handler installation fails.
pub fn install_signal_handlers(shutdown_flag: &ShutdownFlag) -> Result<(), CaravanError> {
    let flag_clone = Arc::clone(shutdown_flag.inner());
    
    ctrlc::set_handler(move || {
        flag_clone.store(true, Ordering::SeqCst);
        eprintln!("\nShutdown requested. Finishing current operation...");
    })
    .map_err(|err| CaravanError::Io(format!("failed to install signal handler: {}", err)))
}

/// Check if shutdown has been requested and return appropriate error.
///
/// If shutdown has been requested, returns `Err(CaravanError::GracefulShutdown)`.
/// Otherwise returns `Ok(())`.
///
/// This is a convenience function for checking the flag in migration loops.
pub fn check_shutdown(shutdown_flag: &ShutdownFlag) -> Result<(), CaravanError> {
    if shutdown_flag.is_shutdown_requested() {
        Err(CaravanError::GracefulShutdown)
    } else {
        Ok(())
    }
}