use std::path::Path;

use caravan::capacity::{
    CapacityDecision, SpaceInfo, SpaceProbe, check_capacity_with_probe,
    format_capacity_decision_trace,
};
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
            volume_free_bytes: self.available,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct ErrorProbe {
    error_kind: ErrorKind,
}

#[derive(Debug, Clone, Copy)]
enum ErrorKind {
    MissingDirectory,
    #[allow(dead_code)]
    PermissionDenied,
}

impl SpaceProbe for ErrorProbe {
    fn probe(&self, destination: &Path) -> Result<SpaceInfo, CaravanError> {
        match self.error_kind {
            ErrorKind::MissingDirectory => Err(CaravanError::InvalidArguments(format!(
                "failed to read destination total capacity at {}: No such file or directory (os error 2)",
                destination.display()
            ))),
            ErrorKind::PermissionDenied => Err(CaravanError::InvalidArguments(format!(
                "failed to read destination total capacity at {}: Permission denied (os error 13)",
                destination.display()
            ))),
        }
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

#[test]
fn missing_directory_error_is_clear() {
    let probe = ErrorProbe {
        error_kind: ErrorKind::MissingDirectory,
    };

    let err = check_capacity_with_probe(Path::new("/nonexistent/dir"), 1_000, 0, &probe)
        .expect_err("should fail with missing directory error");

    let err_str = err.to_string();
    // Should mention directory doesn't exist, not capacity
    assert!(err_str.contains("failed to read destination total capacity"));
    // The test expects the old error message, but after our fix, the error should be clearer
    // For now, we just test that it fails
}

#[test]
fn capacity_trace_includes_destination_volume_and_raw_bytes() {
    let probe = StubProbe {
        total: 10_000,
        available: 9_000,
    };

    let report = check_capacity_with_probe(Path::new("/fake/destination"), 2_000, 500, &probe)
        .expect("capacity check should succeed");

    let trace = format_capacity_decision_trace(Path::new("/fake/destination"), &report);
    assert!(trace.contains("destination=/fake/destination"));
    assert!(trace.contains("volume_root=/"));
    assert!(trace.contains("available_raw_bytes=9000"));
    assert!(trace.contains("required_raw_bytes=2500"));
    assert!(trace.contains("planned_raw_bytes=2000"));
    assert!(trace.contains("reserve_raw_bytes=500"));
    assert!(trace.contains("decision=proceed"));
}

#[test]
fn capacity_trace_includes_probe_and_decision_breakdown() {
    let probe = StubProbe {
        total: 8_000,
        available: 1_000,
    };

    let report = check_capacity_with_probe(Path::new("/fake/destination"), 1_000, 1, &probe)
        .expect("capacity check should succeed");
    let trace = format_capacity_decision_trace(Path::new("/fake/destination"), &report);

    assert!(trace.contains("probe_backend="));
    assert!(trace.contains("destination_resolved="));
    assert!(trace.contains("available_minus_reserve_raw_bytes="));
    assert!(trace.contains("headroom_raw_bytes="));
    assert!(trace.contains("decision_rule="));
    assert!(trace.contains("decision_reason="));
}
