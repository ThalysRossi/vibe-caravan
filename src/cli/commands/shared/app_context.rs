use std::path::Path;

use crate::error::CaravanError;
use crate::migration_registry::{self, MigrationStatus};

pub(crate) struct AppContext {
    registry_path: std::path::PathBuf,
}

impl AppContext {
    pub(crate) fn new() -> Self {
        Self {
            registry_path: migration_registry::default_registry_path(),
        }
    }

    pub(crate) fn register_or_reuse_migration(
        &self,
        source: &str,
        destination: &str,
        mode: &str,
        state_filename: &str,
    ) -> Result<(u64, bool), CaravanError> {
        let mut registry = migration_registry::MigrationRegistry::load(&self.registry_path)?;
        if let Some(existing) = registry.find_by_source_dest(source, destination, mode) {
            return Ok((existing.id, false));
        }

        let id = registry.add_migration(source, destination, mode, state_filename);
        registry.save(&self.registry_path)?;
        Ok((id, true))
    }

    pub(crate) fn persist_migration_status(
        &self,
        migration_id: u64,
        status: MigrationStatus,
    ) -> Result<(), CaravanError> {
        migration_registry::persist_status_transition_with_intent(
            &self.registry_path,
            migration_id,
            status,
        )
    }

    pub(crate) fn find_active_migration_id_for_state(
        &self,
        source: &str,
        destination: &str,
        mode: &str,
        state_path: &Path,
    ) -> Result<Option<u64>, CaravanError> {
        let registry = migration_registry::MigrationRegistry::load(&self.registry_path)?;
        let requested_state_file = state_path.file_name().and_then(|name| name.to_str());

        let matching_incomplete = |entry: &migration_registry::MigrationEntry| {
            entry.source == source
                && entry.destination == destination
                && entry.mode == mode
                && entry.effective_status() != MigrationStatus::Completed
                && entry.effective_status() != MigrationStatus::Failed
        };

        let exact_file_match = requested_state_file.and_then(|state_file| {
            registry
                .migrations
                .iter()
                .rev()
                .find(|entry| matching_incomplete(entry) && entry.state_file == state_file)
                .map(|entry| entry.id)
        });
        if exact_file_match.is_some() {
            return Ok(exact_file_match);
        }

        Ok(registry
            .migrations
            .iter()
            .rev()
            .find(|entry| matching_incomplete(entry))
            .map(|entry| entry.id))
    }
}
