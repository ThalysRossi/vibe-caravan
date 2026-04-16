use caravan::migration_registry::{
    check_source_writable, generate_state_filename, state_dir_in_source, state_file_in_source,
    MigrationRegistry, MigrationStatus,
};
use tempfile::TempDir;

#[test]
fn load_nonexistent_registry_returns_empty() {
    let tmp = TempDir::new().expect("temp dir");
    let registry_path = tmp.path().join("registry.json");

    let registry = MigrationRegistry::load(&registry_path).expect("should load empty registry");
    assert_eq!(registry.migrations.len(), 0);
    assert_eq!(registry.next_id, 1);
}

#[test]
fn save_and_load_registry_round_trip() {
    let tmp = TempDir::new().expect("temp dir");
    let registry_path = tmp.path().join("registry.json");

    let mut registry = MigrationRegistry::new();
    let id1 = registry.add_migration("/source1", "/dest1", "staging", "state1.json");
    let id2 = registry.add_migration("/source2", "/dest2", "migrate", "state2.json");

    registry
        .update_status(id1, MigrationStatus::Running)
        .expect("should update status");
    registry
        .update_status(id2, MigrationStatus::Completed)
        .expect("should update status");

    registry.save(&registry_path).expect("should save registry");

    let loaded = MigrationRegistry::load(&registry_path).expect("should load registry");
    assert_eq!(loaded.migrations.len(), 2);
    assert_eq!(loaded.next_id, 3);

    let m1 = loaded.find_by_id(id1).expect("migration 1 should exist");
    assert_eq!(m1.source, "/source1");
    assert_eq!(m1.destination, "/dest1");
    assert_eq!(m1.mode, "staging");
    assert_eq!(m1.status, MigrationStatus::Running);

    let m2 = loaded.find_by_id(id2).expect("migration 2 should exist");
    assert_eq!(m2.status, MigrationStatus::Completed);
}

#[test]
fn add_migration_increments_id() {
    let mut registry = MigrationRegistry::new();

    let id1 = registry.add_migration("/src1", "/dst1", "staging", "state1.json");
    assert_eq!(id1, 1);
    assert_eq!(registry.next_id, 2);

    let id2 = registry.add_migration("/src2", "/dst2", "migrate", "state2.json");
    assert_eq!(id2, 2);
    assert_eq!(registry.next_id, 3);

    assert_eq!(registry.migrations.len(), 2);
}

#[test]
fn find_first_incomplete_with_multiple_statuses() {
    let mut registry = MigrationRegistry::new();

    let id1 = registry.add_migration("/src1", "/dst1", "staging", "state1.json");
    let id2 = registry.add_migration("/src2", "/dst2", "migrate", "state2.json");
    let id3 = registry.add_migration("/src3", "/dst3", "staging", "state3.json");

    registry
        .update_status(id1, MigrationStatus::Completed)
        .expect("should update");
    registry
        .update_status(id2, MigrationStatus::Running)
        .expect("should update");
    registry
        .update_status(id3, MigrationStatus::AwaitingDeletion)
        .expect("should update");

    let incomplete = registry
        .find_first_incomplete()
        .expect("should find incomplete");
    assert_eq!(incomplete.id, id2); // Running is incomplete
}

#[test]
fn find_by_source_dest_finds_existing_migration() {
    let mut registry = MigrationRegistry::new();

    let id1 = registry.add_migration("/source/a", "/dest/a", "staging", "state1.json");
    let _id2 = registry.add_migration("/source/b", "/dest/b", "migrate", "state2.json");

    // Should find exact match
    let found = registry
        .find_by_source_dest("/source/a", "/dest/a", "staging")
        .expect("should find migration");
    assert_eq!(found.id, id1);

    // Different mode shouldn't match
    let not_found = registry.find_by_source_dest("/source/a", "/dest/a", "migrate");
    assert!(not_found.is_none(), "different mode shouldn't match");

    // Different paths shouldn't match
    let not_found2 = registry.find_by_source_dest("/source/c", "/dest/c", "staging");
    assert!(not_found2.is_none(), "different paths shouldn't match");
}

#[test]
fn find_by_source_dest_finds_incomplete_first() {
    let mut registry = MigrationRegistry::new();

    // Add two migrations with same source/dest but different status
    let id1 = registry.add_migration("/same/source", "/same/dest", "staging", "state1.json");
    registry
        .update_status(id1, MigrationStatus::Completed)
        .expect("should update");

    let id2 = registry.add_migration("/same/source", "/same/dest", "staging", "state2.json");
    registry
        .update_status(id2, MigrationStatus::Running)
        .expect("should update");

    // Should find the incomplete one (Running) first
    let found = registry
        .find_by_source_dest("/same/source", "/same/dest", "staging")
        .expect("should find migration");
    assert_eq!(found.id, id2, "should find incomplete migration first");
    assert_eq!(found.status, MigrationStatus::Running);
}

#[test]
fn find_first_incomplete() {
    let mut registry = MigrationRegistry::new();

    let id1 = registry.add_migration("/src1", "/dst1", "staging", "state1.json");
    let id2 = registry.add_migration("/src2", "/dst2", "migrate", "state2.json");

    registry
        .update_status(id1, MigrationStatus::Completed)
        .expect("should update");
    registry
        .update_status(id2, MigrationStatus::Running)
        .expect("should update");

    let incomplete = registry
        .find_first_incomplete()
        .expect("should find incomplete");
    assert_eq!(incomplete.id, id2);
    assert_eq!(incomplete.status, MigrationStatus::Running);

    registry
        .update_status(id2, MigrationStatus::Completed)
        .expect("should update");
    assert!(registry.find_first_incomplete().is_none());
}

#[test]
fn generate_state_filename_deterministic() {
    let filename1 = generate_state_filename("/source/path", "/dest/path");
    let filename2 = generate_state_filename("/source/path", "/dest/path");
    let filename3 = generate_state_filename("/different/source", "/different/dest");

    assert_eq!(filename1, filename2);
    assert_ne!(filename1, filename3);

    // Should end with .json and start with migration_
    assert!(filename1.starts_with("migration_"));
    assert!(filename1.ends_with(".json"));
}

#[test]
fn state_dir_in_source_creates_correct_path() {
    let tmp = TempDir::new().expect("temp dir");
    let source = tmp.path().join("my_source");

    let state_dir = state_dir_in_source(&source);
    assert_eq!(state_dir, source.join(".caravan"));
}

#[test]
fn state_file_in_source_creates_correct_path() {
    let tmp = TempDir::new().expect("temp dir");
    let source = tmp.path().join("my_source");
    let dest = tmp.path().join("my_dest");

    let state_file = state_file_in_source(&source, &dest);
    let expected_filename =
        generate_state_filename(&source.to_string_lossy(), &dest.to_string_lossy());
    assert_eq!(state_file, source.join(".caravan").join(expected_filename));
}

#[test]
fn check_source_writable_succeeds_for_writable_dir() {
    let tmp = TempDir::new().expect("temp dir");
    let source = tmp.path().join("writable_source");
    std::fs::create_dir_all(&source).expect("should create dir");

    let result = check_source_writable(&source);
    assert!(result.is_ok(), "writable directory should pass check");

    // Verify .caravan directory was created
    let caravan_dir = source.join(".caravan");
    assert!(caravan_dir.exists(), ".caravan directory should exist");
}

#[test]
fn check_source_writable_fails_for_unwritable_dir() {
    let tmp = TempDir::new().expect("temp dir");
    let source_file = tmp.path().join("source-is-a-file");
    std::fs::write(&source_file, "not a directory").expect("create source file");

    let result = check_source_writable(&source_file);
    assert!(
        result.is_err(),
        "non-directory source path should fail writability checks"
    );
    let err = result.expect_err("expected writability failure");
    assert!(
        err.to_string()
            .contains("cannot write state to source directory"),
        "error should explain source state path is not writable: {err}"
    );
}
