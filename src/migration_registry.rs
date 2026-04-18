use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::atomic_write;
use crate::error::CaravanError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationStatus {
    NotStarted,
    Running,
    Verifying,
    AwaitingDeletion,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationEntry {
    pub id: u64,
    pub source: String,
    pub destination: String,
    pub mode: String, // "staging" or "migrate"
    pub status: MigrationStatus,
    #[serde(default)]
    pub pending_status: Option<MigrationStatus>,
    pub created_at: u64,
    pub updated_at: u64,
    pub state_file: String, // e.g., "migration_source_hash_dest_hash.json"
}

impl MigrationEntry {
    pub fn effective_status(&self) -> MigrationStatus {
        self.pending_status.unwrap_or(self.status)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationRegistry {
    pub migrations: Vec<MigrationEntry>,
    pub next_id: u64,
}

impl MigrationRegistry {
    pub fn new() -> Self {
        Self {
            migrations: Vec::new(),
            next_id: 1,
        }
    }

    pub fn load(registry_path: &Path) -> Result<Self, CaravanError> {
        if !registry_path.exists() {
            return Ok(Self::new());
        }

        let content = fs::read_to_string(registry_path).map_err(|err| CaravanError::IoContext {
            context: format!(
                "failed to read migration registry {}",
                registry_path.display()
            ),
            source: err,
        })?;

        serde_json::from_str(&content).map_err(|err| {
            CaravanError::StateCorrupt(format!(
                "failed to parse migration registry {}: {err}",
                registry_path.display()
            ))
        })
    }

    pub fn save(&self, registry_path: &Path) -> Result<(), CaravanError> {
        let content = serde_json::to_string_pretty(self).map_err(|err| {
            CaravanError::StateCorrupt(format!("failed to serialize migration registry: {err}"))
        })?;

        atomic_write::write_bytes(registry_path, content.as_bytes(), "migration registry")
    }

    pub fn find_by_id(&self, id: u64) -> Option<&MigrationEntry> {
        self.migrations.iter().find(|m| m.id == id)
    }

    pub fn find_by_id_mut(&mut self, id: u64) -> Option<&mut MigrationEntry> {
        self.migrations.iter_mut().find(|m| m.id == id)
    }

    pub fn find_by_source_dest(
        &self,
        source: &str,
        dest: &str,
        mode: &str,
    ) -> Option<&MigrationEntry> {
        self.migrations.iter().find(|m| {
            let status = m.effective_status();
            m.source == source
                && m.destination == dest
                && m.mode == mode
                && status != MigrationStatus::Completed
                && status != MigrationStatus::Failed
        })
    }

    pub fn find_first_incomplete(&self) -> Option<&MigrationEntry> {
        self.migrations.iter().find(|m| {
            let status = m.effective_status();
            status != MigrationStatus::Completed && status != MigrationStatus::Failed
        })
    }

    pub fn add_migration(
        &mut self,
        source: &str,
        destination: &str,
        mode: &str,
        state_file: &str,
    ) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let id = self.next_id;
        self.next_id += 1;

        let entry = MigrationEntry {
            id,
            source: source.to_string(),
            destination: destination.to_string(),
            mode: mode.to_string(),
            status: MigrationStatus::NotStarted,
            pending_status: None,
            created_at: now,
            updated_at: now,
            state_file: state_file.to_string(),
        };

        self.migrations.push(entry);
        id
    }

    pub fn update_status(&mut self, id: u64, status: MigrationStatus) -> Result<(), CaravanError> {
        self.begin_status_transition(id, status)?;
        self.commit_status_transition(id)
    }

    pub fn begin_status_transition(
        &mut self,
        id: u64,
        target_status: MigrationStatus,
    ) -> Result<(), CaravanError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(entry) = self.find_by_id_mut(id) {
            entry.pending_status = Some(target_status);
            entry.updated_at = now;
            Ok(())
        } else {
            Err(CaravanError::StateCorrupt(format!(
                "migration with id {} not found",
                id
            )))
        }
    }

    pub fn commit_status_transition(&mut self, id: u64) -> Result<(), CaravanError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(entry) = self.find_by_id_mut(id) {
            if let Some(target_status) = entry.pending_status.take() {
                entry.status = target_status;
            }
            entry.updated_at = now;
            Ok(())
        } else {
            Err(CaravanError::StateCorrupt(format!(
                "migration with id {} not found",
                id
            )))
        }
    }
}

impl Default for MigrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a deterministic filename for migration state based on source and destination paths
pub fn generate_state_filename(source: &str, dest: &str) -> String {
    let normalized_source = normalize_path_for_state_hash(source);
    let normalized_dest = normalize_path_for_state_hash(dest);

    let mut hasher = blake3::Hasher::new();
    hasher.update(normalized_source.as_bytes());
    hasher.update(&[0x00]);
    hasher.update(normalized_dest.as_bytes());
    let hash_hex = hasher.finalize().to_hex();
    let short_hash = &hash_hex[..16];
    format!("migration_{short_hash}.json")
}

fn normalize_path_for_state_hash(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    #[cfg(target_os = "windows")]
    {
        return normalized.to_ascii_lowercase();
    }
    #[cfg(target_os = "linux")]
    {
        normalized
    }
}

/// Get the default registry path in current directory
pub fn default_registry_path() -> PathBuf {
    PathBuf::from(".caravan/migrations.json")
}

/// Get the state directory path within source directory
pub fn state_dir_in_source(source: &Path) -> PathBuf {
    source.join(".caravan")
}

/// Get the full state file path in source directory
pub fn state_file_in_source(source: &Path, dest: &Path) -> PathBuf {
    let state_dir = state_dir_in_source(source);
    let filename = generate_state_filename(&source.to_string_lossy(), &dest.to_string_lossy());
    state_dir.join(filename)
}

/// Check if we can write to the source directory for state files
pub fn check_source_writable(source: &Path) -> Result<(), CaravanError> {
    let state_dir = state_dir_in_source(source);

    // Try to create the directory if it doesn't exist
    if !state_dir.exists() {
        fs::create_dir_all(&state_dir).map_err(|io_source| CaravanError::IoContext {
            context: format!(
                "cannot write state to source directory {}: need write access",
                source.display()
            ),
            source: io_source,
        })?;
    }

    // Try to write a test file
    let test_file = state_dir.join(".write_test");
    fs::write(&test_file, "test").map_err(|io_source| CaravanError::IoContext {
        context: format!(
            "cannot write state to source directory {}: need write access",
            source.display()
        ),
        source: io_source,
    })?;

    // Clean up test file
    let _ = fs::remove_file(&test_file);

    Ok(())
}

/// Persist a migration status update using an explicit recoverable intent protocol.
///
/// If a crash occurs after the intent is saved but before commit, the registry will retain
/// `pending_status`, which callers can interpret via `MigrationEntry::effective_status()`.
pub fn persist_status_transition_with_intent(
    registry_path: &Path,
    migration_id: u64,
    target_status: MigrationStatus,
) -> Result<(), CaravanError> {
    let mut registry = MigrationRegistry::load(registry_path)?;
    registry.begin_status_transition(migration_id, target_status)?;
    registry.save(registry_path)?;

    let mut registry = MigrationRegistry::load(registry_path)?;
    registry.commit_status_transition(migration_id)?;
    registry.save(registry_path)
}
