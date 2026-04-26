use std::path::Path;

use crate::capacity;
use crate::error::CaravanError;

pub(crate) fn ensure_destination_capacity(
    dest: &Path,
    required_bytes: u64,
) -> Result<(), CaravanError> {
    let capacity_report = capacity::check_capacity(dest, required_bytes, 0)?;
    eprintln!(
        "[CAPACITY] {}",
        capacity::format_capacity_decision_trace(dest, &capacity_report)
    );
    if capacity_report.decision == capacity::CapacityDecision::Abort {
        eprintln!(
            "Capacity check failed: {}",
            capacity_report.reason.unwrap_or_default()
        );
        return Err(CaravanError::PolicyBlocked(
            "insufficient destination space".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_destination_capacity_allows_small_required_bytes() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        ensure_destination_capacity(tmp.path(), 1).expect("small requirement should pass");
    }

    #[test]
    fn ensure_destination_capacity_blocks_when_requirement_is_unreachable() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let err = ensure_destination_capacity(tmp.path(), u64::MAX)
            .expect_err("max requirement should fail");
        assert!(err.to_string().contains("insufficient destination space"));
    }
}
