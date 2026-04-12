use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

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
    pub created_at: u64,
    pub updated_at: u64,
    pub state_file: String, // e.g., "migration_source_hash_dest_hash.json"
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

        let content = fs::read_to_string(registry_path).map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "failed to read migration registry {}: {err}",
                registry_path.display()
            ))
        })?;

        serde_json::from_str(&content).map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "failed to parse migration registry {}: {err}",
                registry_path.display()
            ))
        })
    }

    pub fn save(&self, registry_path: &Path) -> Result<(), CaravanError> {
        if let Some(parent) = registry_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                CaravanError::InvalidArguments(format!(
                    "failed to create registry directory {}: {err}",
                    parent.display()
                ))
            })?;
        }

        let content = serde_json::to_string_pretty(self).map_err(|err| {
            CaravanError::InvalidArguments(format!("failed to serialize migration registry: {err}"))
        })?;

        fs::write(registry_path, content).map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "failed to write migration registry {}: {err}",
                registry_path.display()
            ))
        })
    }

    pub fn find_by_id(&self, id: u64) -> Option<&MigrationEntry> {
        self.migrations.iter().find(|m| m.id == id)
    }

    pub fn find_by_id_mut(&mut self, id: u64) -> Option<&mut MigrationEntry> {
        self.migrations.iter_mut().find(|m| m.id == id)
    }

    pub fn find_by_source_dest(&self, source: &str, dest: &str, mode: &str) -> Option<&MigrationEntry> {
        self.migrations
            .iter()
            .find(|m| m.source == source && m.destination == dest && m.mode == mode 
                && m.status != MigrationStatus::Completed && m.status != MigrationStatus::Failed)
    }

    pub fn find_first_incomplete(&self) -> Option<&MigrationEntry> {
        self.migrations
            .iter()
            .find(|m| m.status != MigrationStatus::Completed && m.status != MigrationStatus::Failed)
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
            created_at: now,
            updated_at: now,
            state_file: state_file.to_string(),
        };

        self.migrations.push(entry);
        id
    }

    pub fn update_status(&mut self, id: u64, status: MigrationStatus) -> Result<(), CaravanError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(entry) = self.find_by_id_mut(id) {
            entry.status = status;
            entry.updated_at = now;
            Ok(())
        } else {
            Err(CaravanError::InvalidArguments(format!(
                "migration with id {} not found",
                id
            )))
        }
    }
}

/// Generate a deterministic filename for migration state based on source and destination paths
pub fn generate_state_filename(source: &str, dest: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    dest.hash(&mut hasher);
    let hash = hasher.finish();
    
    // Use first 8 hex digits for brevity
    format!("migration_{:08x}.json", hash & 0xFFFFFFFF)
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
    let filename = generate_state_filename(
        &source.to_string_lossy(),
        &dest.to_string_lossy(),
    );
    state_dir.join(filename)
}

/// Check if we can write to the source directory for state files
pub fn check_source_writable(source: &Path) -> Result<(), CaravanError> {
    let state_dir = state_dir_in_source(source);
    
    // Try to create the directory if it doesn't exist
    if !state_dir.exists() {
        fs::create_dir_all(&state_dir).map_err(|err| {
            CaravanError::InvalidArguments(format!(
                "cannot write state to source directory {}: need write access. Error: {err}",
                source.display()
            ))
        })?;
    }
    
    // Try to write a test file
    let test_file = state_dir.join(".write_test");
    fs::write(&test_file, "test").map_err(|err| {
        CaravanError::InvalidArguments(format!(
            "cannot write state to source directory {}: need write access. Error: {err}",
            source.display()
        ))
    })?;
    
    // Clean up test file
    let _ = fs::remove_file(&test_file);
    
    Ok(())
}