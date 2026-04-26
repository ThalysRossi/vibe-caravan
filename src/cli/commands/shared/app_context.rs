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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration_registry::MigrationRegistry;

    #[test]
    fn register_or_reuse_migration_reuses_active_entry() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let registry_path = tmp.path().join("migrations.json");
        let app = AppContext {
            registry_path: registry_path.clone(),
        };

        let first = app
            .register_or_reuse_migration("/src", "/dst", "staging", "state-a.json")
            .expect("first registration should succeed");
        let second = app
            .register_or_reuse_migration("/src", "/dst", "staging", "state-b.json")
            .expect("second registration should reuse");

        assert_eq!(first.0, second.0);
        assert!(first.1, "first call should create");
        assert!(!second.1, "second call should reuse");
    }

    #[test]
    fn find_active_migration_prefers_exact_state_file_match() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let registry_path = tmp.path().join("migrations.json");
        let mut registry = MigrationRegistry::new();
        let first_id = registry.add_migration("/src", "/dst", "staging", "state-old.json");
        let second_id = registry.add_migration("/src", "/dst", "staging", "state-new.json");
        assert_ne!(first_id, second_id);
        registry.save(&registry_path).expect("save registry");

        let app = AppContext {
            registry_path: registry_path.clone(),
        };
        let state_path = tmp.path().join("state-old.json");
        let match_id = app
            .find_active_migration_id_for_state("/src", "/dst", "staging", &state_path)
            .expect("query should succeed");

        assert_eq!(match_id, Some(first_id));
    }
}
