use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use crate::config::{Config, Mode, TransferConfig, VerificationMode};
use crate::error::CaravanError;
use crate::models::state::{BatchPhase, BatchState, MigrationPhase, MigrationState};
use crate::plan::PlanOptions;
use crate::signal::{ShutdownFlag, check_shutdown, install_signal_handlers};
use crate::{capacity, cleanup, format, migration_registry, plan, prompt, resume, state_store, transfer, verify};
use crate::models;
use crate::prompt::PromptBackend;

/// Save state to both primary (source directory) and secondary (current directory) locations
fn persist_state_both_locations(
    primary_path: &Path,
    secondary_path: &Path,
    state: &MigrationState,
) -> Result<(), CaravanError> {
    // Save to primary location (source directory)
    state_store::persist_state(primary_path, state)?;
    
    // Save to secondary location (current directory) for backward compatibility
    state_store::persist_state(secondary_path, state)?;
    
    Ok(())
}

#[derive(Debug, Parser)]
#[command(name = "caravan", version, about = "Safe staged data migration tool")]
pub struct Cli {
    #[arg(long, default_value = "info")]
    pub log_level: String,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Staging(TransferArgs),
    Migrate(MigrateArgs),
    Status(StateArgs),
    Resume(StateArgs),
}

#[derive(Debug, clap::Args)]
pub struct TransferArgs {
    #[arg(long)]
    pub source: PathBuf,
    #[arg(long)]
    pub dest: PathBuf,
    #[arg(long, value_parser = parse_batch_size)]
    pub batch_size: u64,
    #[arg(long)]
    pub max_files: Option<u64>,
    #[arg(long, default_value_t = false)]
    pub interactive: bool,
    #[arg(long, value_enum, default_value_t = VerificationArg::Digest)]
    pub verification: VerificationArg,
    #[arg(long, default_value_t = false)]
    pub skip_conflicts: bool,
    #[arg(long)]
    pub copy_buffer_size: Option<String>,
    #[arg(long)]
    pub buffered_copy_threshold: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct MigrateArgs {
    #[command(flatten)]
    pub base: TransferArgs,
    #[arg(long)]
    pub snapshot_every: Option<u32>,
}

#[derive(Debug, clap::Args)]
pub struct StateArgs {
    #[arg(long, default_value = ".caravan/state.json")]
    pub state: PathBuf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum VerificationArg {
    Structural,
    Digest,
    Strict,
}

impl From<VerificationArg> for VerificationMode {
    fn from(value: VerificationArg) -> Self {
        match value {
            VerificationArg::Structural => VerificationMode::Structural,
            VerificationArg::Digest => VerificationMode::Digest,
            VerificationArg::Strict => VerificationMode::Strict,
        }
    }
}

pub fn parse_cli_from<I, T>(args: I) -> Result<Config, CaravanError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).map_err(|err| CaravanError::Cli(err.to_string()))?;
    to_config(cli)
}

fn to_config(cli: Cli) -> Result<Config, CaravanError> {
    match cli.command {
        Some(Command::Staging(args)) => {
            validate_transfer_args(&args)?;
            if args.max_files == Some(0) {
                return Err(CaravanError::InvalidArguments(
                    "max-files must be greater than zero".to_string(),
                ));
            }
            let copy_buffer_size = match &args.copy_buffer_size {
                Some(s) => parse_size_usize(s).map_err(|e| CaravanError::InvalidArguments(format!("copy-buffer-size: {}", e)))?,
                None => TransferConfig::default_copy_buffer_size(),
            };
            let buffered_copy_threshold = match &args.buffered_copy_threshold {
                Some(s) => parse_size_u64(s).map_err(|e| CaravanError::InvalidArguments(format!("buffered-copy-threshold: {}", e)))?,
                None => TransferConfig::default_buffered_copy_threshold(),
            };
            Ok(Config::Staging(TransferConfig {
                mode: Mode::Staging,
                source: args.source,
                dest: args.dest,
                batch_size_bytes: args.batch_size,
                max_files: args.max_files,
                snapshot_every: None,
                interactive: args.interactive,
                verification: args.verification.into(),
                log_level: cli.log_level,
                skip_conflicts: args.skip_conflicts,
                copy_buffer_size,
                buffered_copy_threshold,
            }))
        }
        Some(Command::Migrate(args)) => {
            validate_transfer_args(&args.base)?;
            if let Some(0) = args.snapshot_every {
                return Err(CaravanError::InvalidArguments(
                    "snapshot-every must be greater than zero when provided".to_string(),
                ));
            }
            if args.base.max_files == Some(0) {
                return Err(CaravanError::InvalidArguments(
                    "max-files must be greater than zero".to_string(),
                ));
            }
            let copy_buffer_size = match &args.base.copy_buffer_size {
                Some(s) => parse_size_usize(s).map_err(|e| CaravanError::InvalidArguments(format!("copy-buffer-size: {}", e)))?,
                None => TransferConfig::default_copy_buffer_size(),
            };
            let buffered_copy_threshold = match &args.base.buffered_copy_threshold {
                Some(s) => parse_size_u64(s).map_err(|e| CaravanError::InvalidArguments(format!("buffered-copy-threshold: {}", e)))?,
                None => TransferConfig::default_buffered_copy_threshold(),
            };
            Ok(Config::Migrate(TransferConfig {
                mode: Mode::Migrate,
                source: args.base.source,
                dest: args.base.dest,
                batch_size_bytes: args.base.batch_size,
                max_files: args.base.max_files,
                snapshot_every: args.snapshot_every,
                interactive: args.base.interactive,
                verification: args.base.verification.into(),
                log_level: cli.log_level,
                skip_conflicts: args.base.skip_conflicts,
                copy_buffer_size,
                buffered_copy_threshold,
            }))
        }
        Some(Command::Status(args)) => Ok(Config::Status {
            state: args.state,
            log_level: cli.log_level,
        }),
        Some(Command::Resume(args)) => Ok(Config::Resume {
            state: args.state,
            log_level: cli.log_level,
        }),
        None => Err(CaravanError::InvalidArguments("missing subcommand".to_string())),
    }
}

fn validate_transfer_args(args: &TransferArgs) -> Result<(), CaravanError> {
    if args.batch_size == 0 {
        return Err(CaravanError::InvalidArguments(
            "batch-size must be greater than zero".to_string(),
        ));
    }
    if args.source == args.dest {
        return Err(CaravanError::InvalidArguments(
            "source and destination must differ".to_string(),
        ));
    }
    Ok(())
}

pub fn run() -> Result<(), CaravanError> {
    let config = parse_cli_from(std::env::args())?;

    match config {
        Config::Staging(transfer_config) | Config::Migrate(transfer_config) => {
            execute_transfer(transfer_config)
        }
        Config::Status { state, log_level: _ } => {
            execute_status(&state)
        }
        Config::Resume { state, log_level: _ } => {
            execute_resume(&state)
        }
    }
}

pub fn execute_transfer(config: TransferConfig) -> Result<(), CaravanError> {
    // Initialize shutdown flag and install signal handlers
    let shutdown_flag = ShutdownFlag::new();
    install_signal_handlers(&shutdown_flag)?;
    
    // Determine state file paths:
    // 1. Primary: source directory (for resume feature)
    // 2. Secondary: current directory (for backward compatibility)
    let state_path = migration_registry::state_file_in_source(&config.source, &config.dest);
    let secondary_state_path = PathBuf::from(".caravan/state.json");
    
    // Update migration registry
    let registry_path = migration_registry::default_registry_path();
    let mut registry = migration_registry::MigrationRegistry::load(&registry_path)?;
    
    let state_filename = migration_registry::generate_state_filename(
        &config.source.to_string_lossy(),
        &config.dest.to_string_lossy(),
    );
    
    // Check if there's an existing incomplete migration for the same source/destination/mode
    let source_str = config.source.to_string_lossy();
    let dest_str = config.dest.to_string_lossy();
    let mode_str = if config.mode == Mode::Staging { "staging" } else { "migrate" };
    
    let migration_id = if let Some(existing_migration) = registry.find_by_source_dest(&source_str, &dest_str, mode_str) {
        println!("Resuming existing migration ID: {}", existing_migration.id);
        existing_migration.id
    } else {
        // No existing migration found, create a new one
        let new_id = registry.add_migration(
            &source_str,
            &dest_str,
            mode_str,
            &state_filename,
        );
        println!("Migration registered with new ID: {}", new_id);
        new_id
    };
    
    registry.update_status(migration_id, migration_registry::MigrationStatus::Running)?;
    registry.save(&registry_path)?;
    
    // Try to load existing state file, or create new state
    let mut state = if state_path.exists() {
        // Load existing state
        match state_store::load_state(&state_path) {
            Ok(loaded_state) => {
                println!("Loaded existing state from: {}", state_path.display());
                loaded_state
            }
            Err(err) => {
                println!("Warning: Failed to load existing state from {}: {}. Creating new state.", 
                    state_path.display(), err);
                MigrationState::new(mode_str, &source_str, &dest_str)
            }
        }
    } else {
        // No existing state file, create new state
        // Check if source directory is writable before creating new state
        migration_registry::check_source_writable(&config.source)?;
        MigrationState::new(mode_str, &source_str, &dest_str)
    };
    
    state.batch_size_bytes = config.batch_size_bytes;
    
    println!("State will be saved to: {} (primary) and {} (backward compatibility)", 
        state_path.display(), secondary_state_path.display());
    
    // Build plan
    let plan_opts = PlanOptions {
        batch_size_bytes: config.batch_size_bytes,
        max_files: config.max_files.map(|v| v as usize),
    };
    let plan = plan::build_plan(&config.source, &plan_opts)?;

    println!("Planned {} batches for {} files ({} total)",
        plan.batches.len(), plan.source_file_count, format::format_bytes(plan.source_total_bytes));

    // Add ALL batches to state upfront BEFORE processing any
    // Only add batches that don't already exist in state to preserve existing progress
    for batch in &plan.batches {
        if state.batch(&batch.id).is_none() {
            state.upsert_batch(BatchState {
                batch_id: batch.id.clone(),
                phase: BatchPhase::Planned,
                verification_passed: false,
                approved_for_delete: false,
                deleted: false,
            });
        }
        // If batch already exists (e.g., from a previous run), keep its current state
    }
    persist_state_both_locations(&state_path, &secondary_state_path, &state)?;

    let copy_backend = transfer::LocalFsCopyBackend::with_config(
        config.copy_buffer_size,
        config.buffered_copy_threshold,
    );
    let mut processed_batches = 0_u32;

    // === PHASE 1: COPY ALL BATCHES ===
    state.migration_phase = MigrationPhase::Copying;
    persist_state_both_locations(&state_path, &secondary_state_path, &state)?;
    println!("\n=== Copying all batches ===");
    
    for batch in &plan.batches {
        check_shutdown(&shutdown_flag)?;
        
        // Skip batches that are already deleted or already copied
        if let Some(existing_batch) = state.batch(&batch.id) {
            if existing_batch.deleted {
                println!("Skipping {}: already completed", batch.id);
                processed_batches += 1;
                continue;
            }
            if existing_batch.phase == BatchPhase::CopyCompleted || existing_batch.phase == BatchPhase::VerifyCompleted {
                println!("Skipping {}: already copied", batch.id);
                continue;
            }
        }
        
        println!("\n=== Copying {} ({} files, {}) ===", 
            batch.id, batch.file_count, format::format_bytes(batch.total_bytes));
        
        // Initialize batch state if not present
        let mut batch_state = state.batch(&batch.id)
            .cloned()
            .unwrap_or_else(|| BatchState {
                batch_id: batch.id.clone(),
                phase: BatchPhase::Planned,
                verification_passed: false,
                approved_for_delete: false,
                deleted: false,
            });
        
        // Check capacity
        let capacity_report = capacity::check_capacity(&config.dest, batch.total_bytes, 0)?;
        if capacity_report.decision == capacity::CapacityDecision::Abort {
            eprintln!("Capacity check failed: {}", capacity_report.reason.unwrap_or_default());
            return Err(CaravanError::InvalidArguments("insufficient destination space".to_string()));
        }
        
        // Check for naming conflicts
        let conflict_report = crate::conflict::detect_batch_conflicts(batch, &config.dest)?;
        if conflict_report.has_conflicts {
            // Determine whether to skip this batch
            let should_skip = if config.skip_conflicts || !config.interactive {
                // Auto-skip in non-interactive mode or when skip_conflicts flag is set
                true
            } else {
                // Interactive mode: ask user
                let prompt_backend = prompt::InteractivePrompt;
                prompt_backend.confirm_conflict_skip(&batch.id, &conflict_report)?
            };
            
            if should_skip {
                println!("⚠️  Skipping batch '{}' due to {} naming conflict(s)", 
                    batch.id, conflict_report.total_conflicts);
                
                // Mark batch as skipped (we'll treat it as completed to avoid retrying)
                batch_state.phase = BatchPhase::CopyCompleted;
                batch_state.verification_passed = false;
                state.upsert_batch(batch_state.clone());
                persist_state_both_locations(&state_path, &secondary_state_path, &state)?;
                
                // Skip to next batch
                continue;
            }
            // If user chooses not to skip, we'll continue with copy (overwrites files)
            // This is the MVP - we only have skip functionality for now
            // In future implementations, we could proceed with overwrite
        }
        
        // Copy batch
        batch_state.phase = BatchPhase::CopyStarted;
        state.upsert_batch(batch_state.clone());
        persist_state_both_locations(&state_path, &secondary_state_path, &state)?;
        
        let mut progress = crate::progress::TerminalProgress::new();
        transfer::transfer_batch_with_progress(batch, &config.source, &config.dest, &copy_backend, &mut progress)?;
        
        batch_state.phase = BatchPhase::CopyCompleted;
        batch_state.verification_passed = false;
        state.upsert_batch(batch_state.clone());
        persist_state_both_locations(&state_path, &secondary_state_path, &state)?;
    }
    
    // === PHASE 2: VERIFY ALL BATCHES ===
    state.migration_phase = MigrationPhase::Verifying;
    persist_state_both_locations(&state_path, &secondary_state_path, &state)?;
    println!("\n=== Verifying all batches ===");
    
    for batch in &plan.batches {
        check_shutdown(&shutdown_flag)?;
        
        // Skip batches already verified or deleted
        if let Some(existing_batch) = state.batch(&batch.id) {
            if existing_batch.deleted {
                continue;
            }
            if existing_batch.verification_passed && existing_batch.phase == BatchPhase::VerifyCompleted {
                println!("Skipping {}: already verified", batch.id);
                processed_batches += 1;
                continue;
            }
        }
        
        println!("\n=== Verifying {} ({} files, {}) ===",
            batch.id, batch.file_count, format::format_bytes(batch.total_bytes));
        
        let mut batch_state = state.batch(&batch.id)
            .cloned()
            .expect("batch should exist in state");
        
        // Verify batch
        let mut progress = crate::progress::TerminalProgress::new();
        let verification_report = verify::verify_batch_with_progress(
            batch, &config.source, &config.dest, config.verification.clone(), &mut progress
        )?;
        
        batch_state.phase = BatchPhase::VerifyCompleted;
        batch_state.verification_passed = verification_report.status == models::verification::VerificationStatus::Pass;
        state.upsert_batch(batch_state.clone());
        persist_state_both_locations(&state_path, &secondary_state_path, &state)?;
        
        if !batch_state.verification_passed {
            eprintln!("Verification failed: {}", verification_report.recommended_action);
            eprintln!("Missing: {:?}", verification_report.missing_files);
            eprintln!("Mismatched: {:?}", verification_report.mismatched_files);
            return Err(CaravanError::InvalidArguments("verification failed".to_string()));
        }
        
        println!("Verification passed!");
        processed_batches += 1;
    }
    
    // Check for shutdown before proceeding to deletion phase
    check_shutdown(&shutdown_flag)?;
    
    // After all batches are processed, request approval for deletion of all verified batches
    let prompt_backend = prompt::InteractivePrompt;
    let batches_needing_approval = state.batches_needing_approval();
    
    if !batches_needing_approval.is_empty() {
        println!("\n=== All {} batches have been verified successfully ===", batches_needing_approval.len());
        
        let approved = prompt::request_approval_for_batches(
            Some(&prompt_backend),
            config.interactive,
            false,
            &batches_needing_approval,
        )?;
        
        if !approved {
            println!("Deletion not approved. Stopping.");
            return Ok(());
        }
        
        // Mark all batches as approved
        state.approve_batches(&batches_needing_approval);
        persist_state_both_locations(&state_path, &secondary_state_path, &state)?;
        
        // Delete all approved batches
        println!("\n=== Deleting source files for all batches ===");
        for batch_id in &batches_needing_approval {
            // Check for shutdown before each deletion
            check_shutdown(&shutdown_flag)?;
            
            if let Some(batch_state) = state.batch(batch_id) {
                if batch_state.deleted {
                    continue;
                }
                // Load batch definition
                let batch = plan::load_batch_definition(&config.source, batch_id, state.batch_size_bytes)?;
                cleanup::cleanup_batch(&batch, &config.source, &mut state, "execute_transfer")?;
                persist_state_both_locations(&state_path, &secondary_state_path, &state)?;
            }
        }
    }
    
    // Count completed batches (including previously deleted ones)
    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    println!("\n=== Migration complete! {} batches processed, {} total completed ===", 
        processed_batches, completed_count);
    Ok(())
}

fn execute_status(state_path: &Path) -> Result<(), CaravanError> {
    let state = state_store::load_state(state_path)?;
    
    println!("=== Caravan Status ===");
    println!("Mode: {}", state.mode);
    println!("Source: {}", state.source);
    println!("Destination: {}", state.destination);
    println!("Batches: {}", state.batches.len());
    
    for batch in &state.batches {
        println!("  {} - {:?} (verified: {}, approved: {}, deleted: {})",
            batch.batch_id, batch.phase, batch.verification_passed,
            batch.approved_for_delete, batch.deleted);
    }
    
    if let Some(snapshot) = &state.last_successful_snapshot_name {
        println!("Last snapshot: {}", snapshot);
    }
    
    println!("\nJournal entries: {}", state.journal.len());
    for entry in state.journal.iter().rev().take(5) {
        println!("  [{}] {} - {} ({})",
            entry.timestamp_unix_secs, entry.event, entry.batch_id, entry.context);
    }
    
    Ok(())
}

fn execute_resume(state_path: &Path) -> Result<(), CaravanError> {
    // Initialize shutdown flag and install signal handlers
    let shutdown_flag = ShutdownFlag::new();
    install_signal_handlers(&shutdown_flag)?;
    
    let mut state = resume::resume_run(state_path)?;
    
    println!("=== Resuming from saved state ===");
    println!("Mode: {}", state.mode);
    println!("Source: {}", state.source);
    println!("Destination: {}", state.destination);
    println!("Total batches: {}", state.batches.len());
    
    // Count completed batches to calculate resume progress
    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    println!("Completed batches: {} / {}", completed_count, state.batches.len());
    
    // Reconstruct TransferConfig from saved state
    let config = TransferConfig {
        mode: match state.mode.as_str() {
            "staging" => Mode::Staging,
            "migrate" => Mode::Migrate,
            _ => return Err(CaravanError::InvalidArguments(format!("Unknown mode in state: {}", state.mode)))
        },
        source: PathBuf::from(&state.source),
        dest: PathBuf::from(&state.destination),
        batch_size_bytes: 0, // We don't need this for resume, planning is already done
        max_files: None,
        snapshot_every: None,
        interactive: true,
        verification: VerificationMode::Digest,
        log_level: "info".to_string(),
        skip_conflicts: false, // Default to false for resume
        copy_buffer_size: TransferConfig::default_copy_buffer_size(),
        buffered_copy_threshold: TransferConfig::default_buffered_copy_threshold(),
    };
    
    println!("Resuming transfer...\n");
    
    let copy_backend = transfer::LocalFsCopyBackend::with_config(
        config.copy_buffer_size,
        config.buffered_copy_threshold,
    );
    let prompt_backend = prompt::InteractivePrompt;
    
    // Process batches directly from STATE, NOT rebuilding plan
    // Rebuilding plan would generate NEW DIFFERENT batch IDs that don't match existing state
    // which would cause resume to skip all actual work
    // We collect batch IDs first to avoid borrowing issues while mutating state during iteration
    let batch_ids: Vec<String> = state.batches.iter().map(|b| b.batch_id.clone()).collect();
    
    for batch_id in batch_ids {
        // Check for shutdown before starting batch
        check_shutdown(&shutdown_flag)?;
        
        // Clone immediately to release immutable borrow on state
        let batch_state = state.batch(&batch_id)
            .ok_or_else(|| CaravanError::InvalidArguments(
                format!("Batch {} disappeared from state during resume iteration", batch_id)
            ))?
            .clone();
        
        if batch_state.deleted {
            println!("⏭️  Skipping {}: already completed", batch_state.batch_id);
            continue;
        }
        
        // Check current batch phase to decide what to do next
        match batch_state.phase {
            BatchPhase::VerifyCompleted if batch_state.verification_passed => {
                println!("✅ {} already copied & verified, ready for delete", batch_state.batch_id);
            }
            BatchPhase::CopyCompleted => {
                println!("✅ {} already copied, will verify next", batch_state.batch_id);
            }
            BatchPhase::CopyStarted => {
                println!("⚠️  {} partially copied, will retry", batch_state.batch_id);
            }
            phase => {
                println!("🔄 Processing {}: at phase {:?}", batch_state.batch_id, phase);
            }
        }
        
        // We need the actual batch file list to copy/verify
        let mut current_state = batch_state.clone();
        
        // Load original batch definition from disk (IDs are deterministic)
        let batch = plan::load_batch_definition(&config.source, &batch_state.batch_id, state.batch_size_bytes)?;
        
        // Skip verification if already done
        if current_state.phase != BatchPhase::VerifyCompleted {
            // Copy batch if not already completed
            if current_state.phase != BatchPhase::CopyCompleted {
                println!("\n=== Processing {} ({} files, {}) ===", 
                    batch.id, batch.file_count, format::format_bytes(batch.total_bytes));
                
                // Check capacity
                let capacity_report = capacity::check_capacity(&config.dest, batch.total_bytes, 0)?;
                if capacity_report.decision == capacity::CapacityDecision::Abort {
                    eprintln!("Capacity check failed: {}", capacity_report.reason.unwrap_or_default());
                    return Err(CaravanError::InvalidArguments("insufficient destination space".to_string()));
                }
                
                // Copy batch
                current_state.phase = BatchPhase::CopyStarted;
                state.upsert_batch(current_state.clone());
                state_store::persist_state(&state_path, &state)?;
                
                let mut progress = crate::progress::TerminalProgress::new();
                transfer::transfer_batch_with_progress(&batch, &config.source, &config.dest, &copy_backend, &mut progress)?;
                
                current_state.phase = BatchPhase::CopyCompleted;
                state.upsert_batch(current_state.clone());
                state_store::persist_state(&state_path, &state)?;
            }
            
            // Verify batch
            println!("Verifying {}...", batch.id);
            let mut progress = crate::progress::TerminalProgress::new();
            let verification_report = verify::verify_batch_with_progress(
                &batch, &config.source, &config.dest, config.verification.clone(), &mut progress
            )?;
            
            current_state.phase = BatchPhase::VerifyCompleted;
            current_state.verification_passed = verification_report.status == models::verification::VerificationStatus::Pass;
            state.upsert_batch(current_state.clone());
            state_store::persist_state(&state_path, &state)?;
            
            if !current_state.verification_passed {
                eprintln!("Verification failed: {}", verification_report.recommended_action);
                eprintln!("Missing: {:?}", verification_report.missing_files);
                eprintln!("Mismatched: {:?}", verification_report.mismatched_files);
                return Err(CaravanError::InvalidArguments("verification failed".to_string()));
            }
            
            println!("✅ Verification passed!");
        }
    }
    
    // Check for shutdown before proceeding to deletion phase
    check_shutdown(&shutdown_flag)?;
    
    // After processing all batches, request approval for deletion of all verified but not approved batches
    let batches_needing_approval = state.batches_needing_approval();
    if !batches_needing_approval.is_empty() {
        println!("\n=== All {} batches have been verified successfully ===", batches_needing_approval.len());
        
        let approved = prompt::request_approval_for_batches(
            Some(&prompt_backend),
            config.interactive,
            false,
            &batches_needing_approval,
        )?;
        
        if !approved {
            println!("Deletion not approved. Stopping.");
            return Ok(());
        }
        
        // Mark all batches as approved
        state.approve_batches(&batches_needing_approval);
        state_store::persist_state(&state_path, &state)?;
        
        // Delete all approved batches
        println!("\n=== Deleting source files for all batches ===");
        for batch_id in &batches_needing_approval {
            // Check for shutdown before each deletion
            check_shutdown(&shutdown_flag)?;
            
            if let Some(batch_state) = state.batch(batch_id) {
                if batch_state.deleted {
                    continue;
                }
                // Load batch definition
                let batch = plan::load_batch_definition(&config.source, batch_id, state.batch_size_bytes)?;
                cleanup::cleanup_batch(&batch, &config.source, &mut state, "resume")?;
                state_store::persist_state(&state_path, &state)?;
            }
        }
    }
    
    // Count completed batches (including previously deleted ones)
    let completed_count = state.batches.iter().filter(|b| b.deleted).count();
    println!("\n✅ Resume complete! {} batches processed, {} total completed", 
        state.batches.len(), completed_count);
    
    Ok(())
}

fn parse_batch_size(input: &str) -> Result<u64, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("batch-size cannot be empty".to_string());
    }

    let split_idx = raw
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(raw.len());
    let (number, unit_raw) = raw.split_at(split_idx);
    if number.is_empty() {
        return Err("batch-size must start with digits".to_string());
    }

    let base = number
        .parse::<u64>()
        .map_err(|_| "batch-size numeric part is invalid".to_string())?;
    let unit = unit_raw.trim().to_ascii_lowercase();

    let multiplier = match unit.as_str() {
        "" | "b" => 1_u64,
        "kib" => 1024_u64,
        "mib" => 1024_u64.pow(2),
        "gib" => 1024_u64.pow(3),
        "tib" => 1024_u64.pow(4),
        _ => {
            return Err(
                "unsupported batch-size unit; use B, KiB, MiB, GiB, or TiB".to_string(),
            )
        }
    };

    base.checked_mul(multiplier)
        .ok_or_else(|| "batch-size is too large".to_string())
}

fn parse_size_usize(input: &str) -> Result<usize, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("size cannot be empty".to_string());
    }

    let split_idx = raw
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(raw.len());
    let (number, unit_raw) = raw.split_at(split_idx);
    if number.is_empty() {
        return Err("size must start with digits".to_string());
    }

    let base = number
        .parse::<u64>()
        .map_err(|_| "size numeric part is invalid".to_string())?;
    let unit = unit_raw.trim().to_ascii_lowercase();

    let multiplier = match unit.as_str() {
        "" | "b" => 1_u64,
        "kib" => 1024_u64,
        "mib" => 1024_u64.pow(2),
        "gib" => 1024_u64.pow(3),
        "tib" => 1024_u64.pow(4),
        _ => {
            return Err(
                "unsupported size unit; use B, KiB, MiB, GiB, or TiB".to_string(),
            )
        }
    };

    let result = base.checked_mul(multiplier)
        .ok_or_else(|| "size is too large".to_string())?;
    
    // Convert to usize with bounds checking
    if result > usize::MAX as u64 {
        return Err("size exceeds maximum allowed value".to_string());
    }
    
    Ok(result as usize)
}

fn parse_size_u64(input: &str) -> Result<u64, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("size cannot be empty".to_string());
    }

    let split_idx = raw
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(raw.len());
    let (number, unit_raw) = raw.split_at(split_idx);
    if number.is_empty() {
        return Err("size must start with digits".to_string());
    }

    let base = number
        .parse::<u64>()
        .map_err(|_| "size numeric part is invalid".to_string())?;
    let unit = unit_raw.trim().to_ascii_lowercase();

    let multiplier = match unit.as_str() {
        "" | "b" => 1_u64,
        "kib" => 1024_u64,
        "mib" => 1024_u64.pow(2),
        "gib" => 1024_u64.pow(3),
        "tib" => 1024_u64.pow(4),
        _ => {
            return Err(
                "unsupported size unit; use B, KiB, MiB, GiB, or TiB".to_string(),
            )
        }
    };

    base.checked_mul(multiplier)
        .ok_or_else(|| "size is too large".to_string())
}
