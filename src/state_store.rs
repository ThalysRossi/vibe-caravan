use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::atomic_write;
use crate::error::CaravanError;
use crate::models::state::MigrationState;

const STATE_FORMAT_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedStateEnvelope {
    format_version: u32,
    revision: u64,
    state_checksum: String,
    state: MigrationState,
}

#[derive(Debug, Clone)]
struct LoadedStateCandidate {
    state: MigrationState,
    revision: u64,
    state_checksum: String,
}

pub fn persist_state(path: &Path, state: &MigrationState) -> Result<(), CaravanError> {
    let revision = next_revision(path);
    persist_state_with_revision(path, state, revision)
}

/// Persist canonical state and best-effort compatibility backup.
///
/// Canonical state write is mandatory; compatibility backup failures are
/// downgraded to warnings so progress isn't lost due to backup path issues.
pub fn persist_state_with_compat_backup(
    primary_path: &Path,
    secondary_path: &Path,
    state: &MigrationState,
) -> Result<(), CaravanError> {
    let base_revision = next_revision(primary_path).max(next_revision(secondary_path));
    let next_revision = base_revision.saturating_add(1);

    persist_state_with_revision(primary_path, state, next_revision)?;
    if let Err(err) = persist_state_with_revision(secondary_path, state, next_revision) {
        eprintln!(
            "[WARNING] failed to update compatibility backup state at {}: {}. Canonical state at {} remains authoritative.",
            secondary_path.display(),
            err,
            primary_path.display()
        );
    }
    Ok(())
}

pub fn load_state(path: &Path) -> Result<MigrationState, CaravanError> {
    load_state_candidate(path).map(|candidate| candidate.state)
}

pub fn load_state_with_compat_reconciliation(
    primary_path: &Path,
    secondary_path: &Path,
) -> Result<MigrationState, CaravanError> {
    let primary = load_candidate_if_present(primary_path);
    let secondary = load_candidate_if_present(secondary_path);

    match (primary, secondary) {
        (Some(Ok(primary)), Some(Ok(secondary))) => {
            if primary.revision > secondary.revision {
                if primary.state_checksum != secondary.state_checksum {
                    eprintln!(
                        "[WARNING] state divergence detected between canonical {} (rev {}) and backup {} (rev {}); using canonical newer revision.",
                        primary_path.display(),
                        primary.revision,
                        secondary_path.display(),
                        secondary.revision
                    );
                }
                Ok(primary.state)
            } else if secondary.revision > primary.revision {
                eprintln!(
                    "[WARNING] compatibility backup {} is newer (rev {}) than canonical {} (rev {}); recovering from newer valid copy.",
                    secondary_path.display(),
                    secondary.revision,
                    primary_path.display(),
                    primary.revision
                );
                Ok(secondary.state)
            } else if primary.state_checksum == secondary.state_checksum {
                Ok(primary.state)
            } else {
                Err(CaravanError::StateCorrupt(format!(
                    "state divergence detected: canonical {} and backup {} both have revision {} but different checksums",
                    primary_path.display(),
                    secondary_path.display(),
                    primary.revision
                )))
            }
        }
        (Some(Ok(primary)), Some(Err(err))) => {
            eprintln!(
                "[WARNING] compatibility backup state {} is invalid: {}. Continuing with canonical state {}.",
                secondary_path.display(),
                err,
                primary_path.display()
            );
            Ok(primary.state)
        }
        (Some(Err(err)), Some(Ok(secondary))) => {
            eprintln!(
                "[WARNING] canonical state {} is invalid: {}. Recovering from valid compatibility backup {}.",
                primary_path.display(),
                err,
                secondary_path.display()
            );
            Ok(secondary.state)
        }
        (Some(Err(primary_err)), Some(Err(secondary_err))) => Err(CaravanError::StateCorrupt(
            format!(
                "both canonical state {} and compatibility backup {} are invalid (canonical: {}; backup: {})",
                primary_path.display(),
                secondary_path.display(),
                primary_err,
                secondary_err
            ),
        )),
        (Some(Ok(primary)), None) => Ok(primary.state),
        (None, Some(Ok(secondary))) => {
            eprintln!(
                "[WARNING] canonical state {} is missing; recovering from compatibility backup {}.",
                primary_path.display(),
                secondary_path.display()
            );
            Ok(secondary.state)
        }
        (Some(Err(err)), None) => Err(err),
        (None, Some(Err(err))) => Err(err),
        (None, None) => Err(CaravanError::StateRead {
            path: primary_path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no canonical or compatibility state file was found",
            ),
        }),
    }
}

fn next_revision(path: &Path) -> u64 {
    load_candidate_if_present(path)
        .and_then(Result::ok)
        .map_or(1, |candidate| candidate.revision.saturating_add(1))
}

fn persist_state_with_revision(
    path: &Path,
    state: &MigrationState,
    revision: u64,
) -> Result<(), CaravanError> {
    let state_payload = serialize_state(state)?;
    let envelope = PersistedStateEnvelope {
        format_version: STATE_FORMAT_VERSION,
        revision,
        state_checksum: checksum_for_payload(&state_payload),
        state: state.clone(),
    };
    let payload = serde_json::to_vec_pretty(&envelope).map_err(|err| {
        CaravanError::InvalidArguments(format!("failed to serialize state envelope: {err}"))
    })?;

    atomic_write::write_bytes(path, &payload, "state")
}

fn load_candidate_if_present(path: &Path) -> Option<Result<LoadedStateCandidate, CaravanError>> {
    if !path.exists() {
        return None;
    }
    Some(load_state_candidate(path))
}

fn load_state_candidate(path: &Path) -> Result<LoadedStateCandidate, CaravanError> {
    let payload = fs::read_to_string(path).map_err(|source| CaravanError::StateRead {
        path: path.to_path_buf(),
        source,
    })?;

    parse_envelope_or_legacy(path, &payload)
}

fn parse_envelope_or_legacy(
    path: &Path,
    payload: &str,
) -> Result<LoadedStateCandidate, CaravanError> {
    match serde_json::from_str::<PersistedStateEnvelope>(payload) {
        Ok(envelope) => {
            if envelope.format_version != STATE_FORMAT_VERSION {
                return Err(CaravanError::StateCorrupt(format!(
                    "unsupported state format version {} in {}",
                    envelope.format_version,
                    path.display()
                )));
            }

            let state_payload = serialize_state(&envelope.state)?;
            let expected_checksum = checksum_for_payload(&state_payload);
            if envelope.state_checksum != expected_checksum {
                return Err(CaravanError::StateCorrupt(format!(
                    "state checksum mismatch for {}",
                    path.display()
                )));
            }

            Ok(LoadedStateCandidate {
                state: envelope.state,
                revision: envelope.revision,
                state_checksum: expected_checksum,
            })
        }
        Err(_) => {
            let state = serde_json::from_str::<MigrationState>(payload).map_err(|source| {
                CaravanError::StateParse {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            let state_payload = serialize_state(&state)?;
            Ok(LoadedStateCandidate {
                state,
                revision: 0,
                state_checksum: checksum_for_payload(&state_payload),
            })
        }
    }
}

fn serialize_state(state: &MigrationState) -> Result<Vec<u8>, CaravanError> {
    serde_json::to_vec(state)
        .map_err(|err| CaravanError::InvalidArguments(format!("failed to serialize state: {err}")))
}

fn checksum_for_payload(payload: &[u8]) -> String {
    blake3::hash(payload).to_hex().to_string()
}
