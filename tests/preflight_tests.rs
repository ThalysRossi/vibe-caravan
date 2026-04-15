use std::path::PathBuf;

use caravan::config::{CopyStrategy, Mode, TransferConfig, VerificationMode};
use caravan::models::batch::Batch;
use caravan::models::file_entry::FileEntry;
use caravan::plan::PlanningSnapshot;
use caravan::preflight::{
    analyze_staging_preflight_with_probe, DestinationFlags, DestinationProbe,
    DestinationSpaceSnapshot, PreflightWarningCode,
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

fn file(relative: &str, size: u64) -> FileEntry {
    FileEntry {
        relative_path: PathBuf::from(relative),
        size_bytes: size,
        modified_time: None,
    }
}

fn snapshot_with_files(files: Vec<FileEntry>) -> PlanningSnapshot {
    let total = files.iter().map(|f| f.size_bytes).sum::<u64>();
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
        interactive: false,
        verification: VerificationMode::Digest,
        log_level: "info".to_string(),
        skip_conflicts: false,
        recover_failed: false,
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
        .any(|w| w.code == PreflightWarningCode::DestinationCompressed));
    assert!(report
        .warnings
        .iter()
        .any(|w| w.code == PreflightWarningCode::DestinationReparsePoint));
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
        .any(|w| w.code == PreflightWarningCode::CaseCollisionRisk));
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
        .any(|w| w.code == PreflightWarningCode::PathLengthPressure));
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
        .any(|w| w.code == PreflightWarningCode::SpaceAccountingDivergence));
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
        .all(|w| w.code != PreflightWarningCode::SpaceAccountingDivergence));
}
