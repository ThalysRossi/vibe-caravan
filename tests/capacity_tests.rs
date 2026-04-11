use std::path::Path;

use caravan::capacity::{check_capacity_with_probe, CapacityDecision, SpaceInfo, SpaceProbe};
use caravan::error::CaravanError;

#[derive(Debug, Clone, Copy)]
struct StubProbe {
    total: u64,
    available: u64,
}

impl SpaceProbe for StubProbe {
    fn probe(&self, _destination: &Path) -> Result<SpaceInfo, CaravanError> {
        Ok(SpaceInfo {
            total_bytes: self.total,
            available_bytes: self.available,
        })
    }
}

#[test]
fn free_space_greater_than_batch_size_allows_copy() {
    let probe = StubProbe {
        total: 2_000,
        available: 1_500,
    };

    let report = check_capacity_with_probe(Path::new("/fake"), 1_000, 0, &probe)
        .expect("capacity check should succeed");

    assert_eq!(report.decision, CapacityDecision::Proceed);
    assert_eq!(report.reason, None);
}

#[test]
fn free_space_equal_to_batch_size_aborts() {
    let probe = StubProbe {
        total: 2_000,
        available: 1_000,
    };

    let report = check_capacity_with_probe(Path::new("/fake"), 1_000, 0, &probe)
        .expect("capacity check should succeed");

    assert_eq!(report.decision, CapacityDecision::Abort);
    assert!(report.reason.is_some());
}

#[test]
fn free_space_less_than_batch_size_aborts() {
    let probe = StubProbe {
        total: 2_000,
        available: 999,
    };

    let report = check_capacity_with_probe(Path::new("/fake"), 1_000, 0, &probe)
        .expect("capacity check should succeed");

    assert_eq!(report.decision, CapacityDecision::Abort);
}

#[test]
fn reserve_margin_is_applied_correctly() {
    let probe = StubProbe {
        total: 2_000,
        available: 1_200,
    };

    let report = check_capacity_with_probe(Path::new("/fake"), 1_000, 300, &probe)
        .expect("capacity check should succeed");

    assert_eq!(report.decision, CapacityDecision::Abort);
    assert_eq!(report.reserve_margin_bytes, 300);
}

#[test]
fn capacity_failures_include_a_clear_abort_reason() {
    let probe = StubProbe {
        total: 5_000,
        available: 2_000,
    };

    let report = check_capacity_with_probe(Path::new("/fake"), 2_000, 0, &probe)
        .expect("capacity check should succeed");

    // 1. Verify the capacity decision and numeric values are correct
    assert_eq!(report.decision, CapacityDecision::Abort);
    assert_eq!(report.total_capacity_bytes, 5_000);
    assert_eq!(report.available_free_bytes, 2_000);
    assert_eq!(report.planned_batch_bytes, 2_000);
    assert_eq!(report.reserve_margin_bytes, 0);
    
    // 2. Verify the formatted error message contains the correct values
    let reason = report.reason.expect("abort should include reason");
    assert!(reason.contains("insufficient destination space"));
    // 2000 bytes = 1.95 KiB (2000 / 1024 = 1.953125 ≈ 1.95)
    assert!(reason.contains("available=1.95 KiB"));
    assert!(reason.contains("required=1.95 KiB"));
    assert!(reason.contains("batch=1.95 KiB"));
    assert!(reason.contains("reserve=0 bytes"));
}
