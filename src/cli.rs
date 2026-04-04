use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use crate::config::{Config, Mode, TransferConfig, VerificationMode};
use crate::error::CaravanError;
use crate::models::state::{BatchPhase, BatchState, MigrationState};
use crate::plan::PlanOptions;
use crate::{capacity, cleanup, plan, prompt, resume, state_store, transfer, verify};
use crate::models;

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

fn execute_transfer(config: TransferConfig) -> Result<(), CaravanError> {
    use std::path::Path;
    
    // Initialize state
    let state_path = Path::new(".caravan/state.json");
    let mut state = MigrationState::new(
        if config.mode == Mode::Staging { "staging" } else { "migrate" },
        &config.source.to_string_lossy(),
        &config.dest.to_string_lossy(),
    );
    
    // Build plan
    let plan_opts = PlanOptions {
        batch_size_bytes: config.batch_size_bytes,
        max_files: config.max_files.map(|v| v as usize),
    };
    let plan = plan::build_plan(&config.source, &plan_opts)?;
    
    println!("Planned {} batches for {} files ({} bytes total)",
        plan.batches.len(), plan.source_file_count, plan.source_total_bytes);
    
    // Process each batch
    let copy_backend = transfer::LocalFsCopyBackend;
    let prompt_backend = prompt::InteractivePrompt;
    let mut completed_batches = 0_u32;
    
    for batch in &plan.batches {
        println!("\n=== Processing {} ({} files, {} bytes) ===", 
            batch.id, batch.file_count, batch.total_bytes);
        
        // Initialize batch state
        let mut batch_state = BatchState {
            batch_id: batch.id.clone(),
            phase: BatchPhase::Planned,
            verification_passed: false,
            approved_for_delete: false,
            deleted: false,
        };
        state.upsert_batch(batch_state.clone());
        state_store::persist_state(state_path, &state)?;
        
        // Check capacity
        let capacity_report = capacity::check_capacity(&config.dest, batch.total_bytes, 0)?;
        if capacity_report.decision == capacity::CapacityDecision::Abort {
            eprintln!("Capacity check failed: {}", capacity_report.reason.unwrap_or_default());
            return Err(CaravanError::InvalidArguments("insufficient destination space".to_string()));
        }
        
        // Copy batch
        println!("Copying batch...");
        batch_state.phase = BatchPhase::CopyStarted;
        state.upsert_batch(batch_state.clone());
        state_store::persist_state(state_path, &state)?;
        
        transfer::transfer_batch(batch, &config.source, &config.dest, &copy_backend)?;
        
        batch_state.phase = BatchPhase::CopyCompleted;
        state.upsert_batch(batch_state.clone());
        state_store::persist_state(state_path, &state)?;
        
        // Verify batch
        println!("Verifying batch...");
        let verification_report = verify::verify_batch(
            batch, &config.source, &config.dest, config.verification.clone()
        )?;
        
        batch_state.phase = BatchPhase::VerifyCompleted;
        batch_state.verification_passed = verification_report.status == models::verification::VerificationStatus::Pass;
        state.upsert_batch(batch_state.clone());
        state_store::persist_state(state_path, &state)?;
        
        if !batch_state.verification_passed {
            eprintln!("Verification failed: {}", verification_report.recommended_action);
            eprintln!("Missing: {:?}", verification_report.missing_files);
            eprintln!("Mismatched: {:?}", verification_report.mismatched_files);
            return Err(CaravanError::InvalidArguments("verification failed".to_string()));
        }
        
        println!("Verification passed!");
        
        // Request approval for deletion
        let approved = prompt::request_approval(
            Some(&prompt_backend),
            config.interactive,
            false,
            &batch.id,
        )?;
        
        if !approved {
            println!("Deletion not approved. Stopping.");
            return Ok(());
        }
        
        batch_state.approved_for_delete = true;
        batch_state.phase = BatchPhase::ApprovedForDelete;
        state.upsert_batch(batch_state.clone());
        state_store::persist_state(state_path, &state)?;
        
        // Delete source files
        println!("Deleting source files...");
        cleanup::cleanup_batch(batch, &config.source, &mut state, "execute_transfer")?;
        state_store::persist_state(state_path, &state)?;
        
        completed_batches += 1;
        
        // Snapshot if needed (migrate mode only)
        if config.mode == Mode::Migrate && config.snapshot_every.is_some() {
            println!("Snapshot support requires platform-specific backend implementation");
        }
    }
    
    println!("\n=== Migration complete! Processed {} batches ===", completed_batches);
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
    let state = resume::resume_run(state_path)?;
    
    println!("=== Resuming from saved state ===");
    println!("Mode: {}", state.mode);
    println!("Batches in state: {}", state.batches.len());
    
    // This would need to reconstruct the config and continue from where it left off
    // For now, just show what would be resumed
    for batch in &state.batches {
        if !batch.deleted {
            println!("Would resume: {} at phase {:?}", batch.batch_id, batch.phase);
        }
    }
    
    Err(CaravanError::NotImplemented("full resume logic requires config reconstruction"))
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