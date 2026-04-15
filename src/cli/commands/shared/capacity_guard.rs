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
        return Err(CaravanError::InvalidArguments(
            "insufficient destination space".to_string(),
        ));
    }

    Ok(())
}
