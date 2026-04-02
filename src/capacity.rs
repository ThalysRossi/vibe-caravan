use std::path::Path;

use crate::error::WololoError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapacityDecision {
    Proceed,
    Abort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacityReport {
    pub total_capacity_bytes: u64,
    pub available_free_bytes: u64,
    pub planned_batch_bytes: u64,
    pub reserve_margin_bytes: u64,
    pub decision: CapacityDecision,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

pub trait SpaceProbe {
    fn probe(&self, destination: &Path) -> Result<SpaceInfo, WololoError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemSpaceProbe;

impl SpaceProbe for SystemSpaceProbe {
    fn probe(&self, destination: &Path) -> Result<SpaceInfo, WololoError> {
        let total_bytes = fs2::total_space(destination).map_err(|err| {
            WololoError::InvalidArguments(format!(
                "failed to read destination total capacity at {}: {err}",
                destination.display()
            ))
        })?;
        let available_bytes = fs2::available_space(destination).map_err(|err| {
            WololoError::InvalidArguments(format!(
                "failed to read destination free space at {}: {err}",
                destination.display()
            ))
        })?;

        Ok(SpaceInfo {
            total_bytes,
            available_bytes,
        })
    }
}

pub fn check_capacity(
    destination: &Path,
    planned_batch_bytes: u64,
    reserve_margin_bytes: u64,
) -> Result<CapacityReport, WololoError> {
    let probe = SystemSpaceProbe;
    check_capacity_with_probe(
        destination,
        planned_batch_bytes,
        reserve_margin_bytes,
        &probe,
    )
}

pub fn check_capacity_with_probe(
    destination: &Path,
    planned_batch_bytes: u64,
    reserve_margin_bytes: u64,
    probe: &dyn SpaceProbe,
) -> Result<CapacityReport, WololoError> {
    if planned_batch_bytes == 0 {
        return Err(WololoError::InvalidArguments(
            "planned batch size must be greater than zero".to_string(),
        ));
    }

    let space = probe.probe(destination)?;
    let required_bytes = planned_batch_bytes.saturating_add(reserve_margin_bytes);
    let decision = if space.available_bytes > required_bytes {
        CapacityDecision::Proceed
    } else {
        CapacityDecision::Abort
    };

    let reason = if decision == CapacityDecision::Abort {
        Some(format!(
            "insufficient destination space: available={} required={} (batch={} reserve={})",
            space.available_bytes, required_bytes, planned_batch_bytes, reserve_margin_bytes
        ))
    } else {
        None
    };

    Ok(CapacityReport {
        total_capacity_bytes: space.total_bytes,
        available_free_bytes: space.available_bytes,
        planned_batch_bytes,
        reserve_margin_bytes,
        decision,
        reason,
    })
}
