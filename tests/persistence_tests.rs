use std::fs;

use caravan::models::state::MigrationState;
use caravan::state_store::{load_state, persist_state_with_compat_backup};
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
