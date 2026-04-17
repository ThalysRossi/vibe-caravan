//! Signal handling for graceful shutdown.
//!
//! Provides cross-platform signal handling for graceful shutdown on both Windows and Linux.
//! Uses the `ctrlc` crate internally for cross-platform compatibility.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

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

fn active_shutdown_target() -> &'static Mutex<Option<Arc<AtomicBool>>> {
    static ACTIVE_TARGET: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();
    ACTIVE_TARGET.get_or_init(|| Mutex::new(None))
}

fn install_guard() -> &'static Mutex<()> {
    static INSTALL_GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    INSTALL_GUARD.get_or_init(|| Mutex::new(()))
}

fn update_active_shutdown_target(shutdown_flag: &ShutdownFlag) -> Result<(), CaravanError> {
    let mut guard = active_shutdown_target().lock().map_err(|_| {
        CaravanError::Io("failed to lock active shutdown target due to poisoned mutex".to_string())
    })?;
    *guard = Some(Arc::clone(shutdown_flag.inner()));
    Ok(())
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
    static HANDLER_INSTALLED: OnceLock<()> = OnceLock::new();
    let _install_lock = install_guard().lock().map_err(|_| {
        CaravanError::Io(
            "failed to lock signal installation guard due to poisoned mutex".to_string(),
        )
    })?;

    if HANDLER_INSTALLED.get().is_none() {
        ctrlc::set_handler(move || {
            if let Ok(guard) = active_shutdown_target().lock() {
                if let Some(active_flag) = guard.as_ref() {
                    active_flag.store(true, Ordering::SeqCst);
                }
            }
            eprintln!("\nShutdown requested. Finishing current file...");
        })
        .map_err(|err| CaravanError::Io(format!("failed to install signal handler: {}", err)))?;
        let _ = HANDLER_INSTALLED.set(());
    }

    update_active_shutdown_target(shutdown_flag)
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
