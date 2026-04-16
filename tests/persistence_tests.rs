use std::fs;

use caravan::models::state::MigrationState;
use caravan::state_store::{
    load_state, load_state_with_compat_reconciliation, persist_state,
    persist_state_with_compat_backup,
};
use tempfile::TempDir;

#[test]
fn persists_primary_when_secondary_backup_write_fails() {
    let tmp = TempDir::new().expect("temp dir");
    let primary_path = tmp
        .path()
        .join("source")
        .join(".caravan")
        .join("state.json");
    let secondary_path = tmp.path().join("compat-backup-as-directory");
    fs::create_dir_all(&secondary_path).expect("create conflicting secondary directory");

    let state = MigrationState::new("staging", "/source", "/dest");
    persist_state_with_compat_backup(&primary_path, &secondary_path, &state)
        .expect("primary canonical write should succeed");

    let loaded = load_state(&primary_path).expect("canonical state should be readable");
    assert_eq!(loaded, state);
    assert!(
        secondary_path.is_dir(),
        "secondary path should remain untouched"
    );
}

#[test]
fn persists_secondary_backup_when_available() {
    let tmp = TempDir::new().expect("temp dir");
    let primary_path = tmp
        .path()
        .join("source")
        .join(".caravan")
        .join("state.json");
    let secondary_path = tmp.path().join(".caravan").join("state.json");

    let state = MigrationState::new("migrate", "/source", "/dest");
    persist_state_with_compat_backup(&primary_path, &secondary_path, &state)
        .expect("both writes should succeed");

    let primary_loaded = load_state(&primary_path).expect("load primary state");
    let secondary_loaded = load_state(&secondary_path).expect("load secondary state");
    assert_eq!(primary_loaded, state);
    assert_eq!(secondary_loaded, state);
}

#[test]
fn persisted_state_uses_versioned_envelope_metadata() {
    let tmp = TempDir::new().expect("temp dir");
    let state_path = tmp.path().join("state.json");

    let state = MigrationState::new("staging", "/source", "/dest");
    persist_state(&state_path, &state).expect("persist state");

    let payload = fs::read_to_string(&state_path).expect("read envelope");
    let json: serde_json::Value = serde_json::from_str(&payload).expect("parse envelope json");
    assert_eq!(json["format_version"], 2);
    assert_eq!(json["revision"], 1);
    assert!(json["state_checksum"].is_string());
    assert_eq!(json["state"]["source"], "/source");
}

#[test]
fn reconciliation_prefers_newer_valid_state_copy() {
    let tmp = TempDir::new().expect("temp dir");
    let primary_path = tmp
        .path()
        .join("source")
        .join(".caravan")
        .join("state.json");
    let secondary_path = tmp.path().join(".caravan").join("state.json");

    let mut state_v1 = MigrationState::new("staging", "/source", "/dest");
    state_v1.batch_size_bytes = 1024;
    persist_state_with_compat_backup(&primary_path, &secondary_path, &state_v1)
        .expect("persist both v1");

    let mut state_v2 = state_v1.clone();
    state_v2.batch_size_bytes = 2048;
    persist_state(&secondary_path, &state_v2).expect("persist newer backup state");

    let loaded = load_state_with_compat_reconciliation(&primary_path, &secondary_path)
        .expect("reconcile should prefer newest valid state");
    assert_eq!(loaded.batch_size_bytes, 2048);
}

#[test]
fn reconciliation_fails_on_equal_revision_checksum_divergence() {
    let tmp = TempDir::new().expect("temp dir");
    let primary_path = tmp
        .path()
        .join("source")
        .join(".caravan")
        .join("state.json");
    let secondary_path = tmp.path().join(".caravan").join("state.json");

    let mut primary_state = MigrationState::new("staging", "/source", "/dest");
    primary_state.batch_size_bytes = 1024;
    persist_state(&primary_path, &primary_state).expect("persist primary state");

    let mut secondary_state = primary_state.clone();
    secondary_state.batch_size_bytes = 4096;
    persist_state(&secondary_path, &secondary_state).expect("persist secondary state");

    let err = load_state_with_compat_reconciliation(&primary_path, &secondary_path)
        .expect_err("equal-revision divergence should fail closed");
    assert!(
        err.to_string().contains("state divergence detected"),
        "expected divergence error, got: {err}"
    );
}
