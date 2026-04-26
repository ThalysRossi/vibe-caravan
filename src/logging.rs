use std::sync::OnceLock;

use crate::error::CaravanError;

pub fn init_logging(log_level: &str) -> Result<(), CaravanError> {
    static LOGGING_INIT: OnceLock<()> = OnceLock::new();
    if LOGGING_INIT.get().is_some() {
        return Ok(());
    }

    let level = parse_level(log_level)?;
    let subscriber = tracing_subscriber::fmt()
        .with_target(false)
        .with_max_level(level)
        .finish();

    match tracing::subscriber::set_global_default(subscriber) {
        Ok(()) => {
            let _ = LOGGING_INIT.set(());
            Ok(())
        }
        Err(_) => {
            // If another subscriber is already installed (tests/embedding), treat as initialized.
            let _ = LOGGING_INIT.set(());
            Ok(())
        }
    }
}

fn parse_level(raw: &str) -> Result<tracing::Level, CaravanError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "trace" => Ok(tracing::Level::TRACE),
        "debug" => Ok(tracing::Level::DEBUG),
        "info" => Ok(tracing::Level::INFO),
        "warn" => Ok(tracing::Level::WARN),
        "error" => Ok(tracing::Level::ERROR),
        other => Err(CaravanError::InvalidArguments(format!(
            "unsupported log-level '{other}'; use trace, debug, info, warn, or error"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_level_accepts_trimmed_case_insensitive_values() {
        assert_eq!(
            parse_level("  TRACE ").expect("trace should parse"),
            tracing::Level::TRACE
        );
        assert_eq!(
            parse_level("Debug").expect("debug should parse"),
            tracing::Level::DEBUG
        );
        assert_eq!(
            parse_level("info").expect("info should parse"),
            tracing::Level::INFO
        );
        assert_eq!(
            parse_level("WARN").expect("warn should parse"),
            tracing::Level::WARN
        );
        assert_eq!(
            parse_level("error").expect("error should parse"),
            tracing::Level::ERROR
        );
    }

    #[test]
    fn parse_level_rejects_unknown_levels() {
        let err = parse_level("verbose").expect_err("unknown level must fail");
        let rendered = err.to_string();
        assert!(
            rendered.contains("unsupported log-level 'verbose'"),
            "unexpected message: {rendered}"
        );
    }
}
