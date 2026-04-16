use std::path::PathBuf;

use caravan::config::{CopyStrategy, Mode, TransferConfig};
use caravan::models::batch::Batch;
use caravan::models::file_entry::FileEntry;
use caravan::plan::PlanningSnapshot;
use caravan::preflight::{
    analyze_migrate_preflight_with_probe, analyze_staging_preflight_with_probe,
    analyze_transfer_preflight_with_probes, enforce_transfer_preflight_policy, DestinationFlags,
    DestinationProbe, DestinationSpaceSnapshot, FilesystemTypeProbe, PreflightWarningCode,
};

#[derive(Debug, Clone, Copy)]
struct StubProbe {
    flags: DestinationFlags,
    space: Option<DestinationSpaceSnapshot>,
}

impl DestinationProbe for StubProbe {
    fn destination_flags(
        &self,
        _destination: &std::path::Path,
    ) -> Result<DestinationFlags, caravan::error::CaravanError> {
        Ok(self.flags)
    }

    fn destination_space(
        &self,
        _destination: &std::path::Path,
    ) -> Result<Option<DestinationSpaceSnapshot>, caravan::error::CaravanError> {
        Ok(self.space)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StubFilesystemProbe {
    source_fs: Option<String>,
    dest_fs: Option<String>,
}

impl FilesystemTypeProbe for StubFilesystemProbe {
    fn filesystem_type(
        &self,
        path: &std::path::Path,
    ) -> Result<Option<String>, caravan::error::CaravanError> {
        if path.to_string_lossy() == "/src" {
            return Ok(self.source_fs.clone());
        }
        if path.to_string_lossy() == "/dest" {
            return Ok(self.dest_fs.clone());
        }
        Ok(None)
    }
}

fn file(relative: &str, size: u64) -> FileEntry {
    FileEntry {
        relative_path: PathBuf::from(relative),
        size_bytes: size,
        modified_time: None,
    }
}

fn snapshot_with_files(files: Vec<FileEntry>) -> PlanningSnapshot {
    let total = files
        .iter()
        .map(|file_entry| file_entry.size_bytes)
        .sum::<u64>();
    PlanningSnapshot {
        source_file_count: files.len(),
        source_total_bytes: total,
        batches: vec![Batch {
            id: "batch-000001".to_string(),
            file_count: files.len(),
            total_bytes: total,
            files,
        }],
    }
}

fn staging_config(dest: &str) -> TransferConfig {
    TransferConfig {
        mode: Mode::Staging,
        source: PathBuf::from("/src"),
        dest: PathBuf::from(dest),
        batch_size_bytes: 1024,
        max_files: None,
        snapshot_every: None,
        snapshot_dir: None,
        interactive: false,
        log_level: "info".to_string(),
        skip_conflicts: false,
        recover_failed: false,
        allow_unsafe_filesystems: false,
        copy_strategy: CopyStrategy::Auto,
        copy_buffer_size: TransferConfig::default_copy_buffer_size(),
        buffered_copy_threshold: TransferConfig::default_buffered_copy_threshold(),
    }
}

fn migrate_config(source: &str, dest: &str) -> TransferConfig {
    TransferConfig {
        mode: Mode::Migrate,
        source: PathBuf::from(source),
        dest: PathBuf::from(dest),
        batch_size_bytes: 1024,
        max_files: None,
        snapshot_every: None,
        snapshot_dir: None,
        interactive: false,
        log_level: "info".to_string(),
        skip_conflicts: false,
        recover_failed: false,
        allow_unsafe_filesystems: false,
        copy_strategy: CopyStrategy::Auto,
        copy_buffer_size: TransferConfig::default_copy_buffer_size(),
        buffered_copy_threshold: TransferConfig::default_buffered_copy_threshold(),
    }
}

#[test]
fn warns_when_destination_has_compression_or_reparse_flags() {
    let config = staging_config("/dest");
    let snapshot = snapshot_with_files(vec![file("a.txt", 10)]);
    let probe = StubProbe {
        flags: DestinationFlags {
            is_compressed: true,
            is_reparse_point: true,
        },
        space: None,
    };

    let report = analyze_staging_preflight_with_probe(&config, &snapshot, &probe)
        .expect("preflight should succeed");

    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == PreflightWarningCode::DestinationCompressed));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == PreflightWarningCode::DestinationReparsePoint));
}

#[test]
fn warns_when_case_collisions_exist_in_planned_paths() {
    let config = staging_config("/dest");
    let snapshot = snapshot_with_files(vec![
        file("Movies/File.MKV", 100),
        file("movies/file.mkv", 100),
    ]);
    let probe = StubProbe {
        flags: DestinationFlags {
            is_compressed: false,
            is_reparse_point: false,
        },
        space: None,
    };

    let report = analyze_staging_preflight_with_probe(&config, &snapshot, &probe)
        .expect("preflight should succeed");

    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == PreflightWarningCode::CaseCollisionRisk));
}

#[test]
fn warns_when_estimated_destination_path_length_is_near_windows_limit() {
    let config = staging_config("D:\\Media");
    let long_name = format!("nested\\{}", "x".repeat(245));
    let snapshot = snapshot_with_files(vec![file(&long_name, 1)]);
    let probe = StubProbe {
        flags: DestinationFlags {
            is_compressed: false,
            is_reparse_point: false,
        },
        space: None,
    };

    let report = analyze_staging_preflight_with_probe(&config, &snapshot, &probe)
        .expect("preflight should succeed");

    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == PreflightWarningCode::PathLengthPressure));
}

#[test]
fn warns_when_available_and_volume_free_space_diverge_on_windows_destination() {
    let config = staging_config("D:\\Media");
    let snapshot = snapshot_with_files(vec![file("movie.mkv", 1)]);
    let probe = StubProbe {
        flags: DestinationFlags {
            is_compressed: false,
            is_reparse_point: false,
        },
        space: Some(DestinationSpaceSnapshot {
            total_bytes: 1_000 * 1024 * 1024 * 1024,
            available_bytes: 16 * 1024 * 1024 * 1024,
            volume_free_bytes: 315 * 1024 * 1024 * 1024,
        }),
    };

    let report = analyze_staging_preflight_with_probe(&config, &snapshot, &probe)
        .expect("preflight should succeed");

    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == PreflightWarningCode::SpaceAccountingDivergence));
}

#[test]
fn does_not_warn_when_available_and_volume_free_are_close() {
    let config = staging_config("D:\\Media");
    let snapshot = snapshot_with_files(vec![file("movie.mkv", 1)]);
    let probe = StubProbe {
        flags: DestinationFlags {
            is_compressed: false,
            is_reparse_point: false,
        },
        space: Some(DestinationSpaceSnapshot {
            total_bytes: 1_000 * 1024 * 1024 * 1024,
            available_bytes: 300 * 1024 * 1024 * 1024,
            volume_free_bytes: 315 * 1024 * 1024 * 1024,
        }),
    };

    let report = analyze_staging_preflight_with_probe(&config, &snapshot, &probe)
        .expect("preflight should succeed");

    assert!(report
        .warnings
        .iter()
        .all(|warning| warning.code != PreflightWarningCode::SpaceAccountingDivergence));
}

#[test]
fn migrate_warns_when_source_is_not_ntfs_like() {
    let config = migrate_config("/src", "/dest");
    let probe = StubFilesystemProbe {
        source_fs: Some("ext4".to_string()),
        dest_fs: Some("btrfs".to_string()),
    };

    let report = analyze_migrate_preflight_with_probe(&config, &probe)
        .expect("migrate preflight should succeed");

    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == PreflightWarningCode::SourceFilesystemNotNtfsLike));
    assert!(report
        .warnings
        .iter()
        .all(|warning| warning.code != PreflightWarningCode::DestinationFilesystemNotBtrfs));
}

#[test]
fn migrate_warns_when_destination_is_not_btrfs() {
    let config = migrate_config("/src", "/dest");
    let probe = StubFilesystemProbe {
        source_fs: Some("ntfs3".to_string()),
        dest_fs: Some("ext4".to_string()),
    };

    let report = analyze_migrate_preflight_with_probe(&config, &probe)
        .expect("migrate preflight should succeed");

    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == PreflightWarningCode::DestinationFilesystemNotBtrfs));
    assert!(report
        .warnings
        .iter()
        .all(|warning| warning.code != PreflightWarningCode::SourceFilesystemNotNtfsLike));
}

#[test]
fn migrate_does_not_warn_for_ntfs_like_source_and_btrfs_destination() {
    let config = migrate_config("/src", "/dest");
    let probe = StubFilesystemProbe {
        source_fs: Some("fuseblk".to_string()),
        dest_fs: Some("btrfs".to_string()),
    };

    let report = analyze_migrate_preflight_with_probe(&config, &probe)
        .expect("migrate preflight should succeed");

    assert!(report.warnings.is_empty());
}

#[test]
fn transfer_preflight_includes_migrate_filesystem_warnings() {
    let config = TransferConfig {
        mode: Mode::Migrate,
        source: PathBuf::from("/src"),
        dest: PathBuf::from("/dest"),
        batch_size_bytes: 1024,
        max_files: None,
        snapshot_every: None,
        snapshot_dir: None,
        interactive: false,
        log_level: "info".to_string(),
        skip_conflicts: false,
        recover_failed: false,
        allow_unsafe_filesystems: false,
        copy_strategy: CopyStrategy::Auto,
        copy_buffer_size: TransferConfig::default_copy_buffer_size(),
        buffered_copy_threshold: TransferConfig::default_buffered_copy_threshold(),
    };
    let snapshot = snapshot_with_files(vec![
        file("Movies/File.MKV", 100),
        file("movies/file.mkv", 100),
    ]);
    let destination_probe = StubProbe {
        flags: DestinationFlags {
            is_compressed: false,
            is_reparse_point: false,
        },
        space: None,
    };
    let filesystem_probe = StubFilesystemProbe {
        source_fs: Some("ext4".to_string()),
        dest_fs: Some("xfs".to_string()),
    };

    let report = analyze_transfer_preflight_with_probes(
        &config,
        &snapshot,
        &destination_probe,
        &filesystem_probe,
    )
    .expect("combined preflight should succeed");

    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == PreflightWarningCode::SourceFilesystemNotNtfsLike));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.code == PreflightWarningCode::DestinationFilesystemNotBtrfs));
}

#[test]
fn migrate_preflight_policy_blocks_unsafe_filesystem_topology_by_default() {
    let config = migrate_config("/src", "/dest");
    let snapshot = snapshot_with_files(vec![file("file.txt", 10)]);
    let destination_probe = StubProbe {
        flags: DestinationFlags {
            is_compressed: false,
            is_reparse_point: false,
        },
        space: None,
    };
    let filesystem_probe = StubFilesystemProbe {
        source_fs: Some("ext4".to_string()),
        dest_fs: Some("xfs".to_string()),
    };

    let report = analyze_transfer_preflight_with_probes(
        &config,
        &snapshot,
        &destination_probe,
        &filesystem_probe,
    )
    .expect("combined preflight should succeed");

    let err = enforce_transfer_preflight_policy(&config, &report)
        .expect_err("migrate should fail-closed for unsafe filesystems");
    let message = err.to_string();
    assert!(message.contains("source_filesystem_not_ntfs_like"));
    assert!(message.contains("destination_filesystem_not_btrfs"));
}

#[test]
fn migrate_preflight_policy_can_be_overridden_explicitly() {
    let mut config = migrate_config("/src", "/dest");
    config.allow_unsafe_filesystems = true;
    let snapshot = snapshot_with_files(vec![file("file.txt", 10)]);
    let destination_probe = StubProbe {
        flags: DestinationFlags {
            is_compressed: false,
            is_reparse_point: false,
        },
        space: None,
    };
    let filesystem_probe = StubFilesystemProbe {
        source_fs: Some("ext4".to_string()),
        dest_fs: Some("xfs".to_string()),
    };

    let report = analyze_transfer_preflight_with_probes(
        &config,
        &snapshot,
        &destination_probe,
        &filesystem_probe,
    )
    .expect("combined preflight should succeed");

    enforce_transfer_preflight_policy(&config, &report)
        .expect("override flag should bypass fail-closed policy");
}
