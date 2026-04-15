use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::config::{Config, Mode, TransferConfig, VerificationMode};
use crate::error::CaravanError;

mod args;
mod commands;

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
    #[arg(long, value_parser = args::parse_batch_size)]
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

            let (copy_buffer_size, buffered_copy_threshold) = args::parse_copy_options(
                args.copy_buffer_size.as_deref(),
                args.buffered_copy_threshold.as_deref(),
            )?;

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

            let (copy_buffer_size, buffered_copy_threshold) = args::parse_copy_options(
                args.base.copy_buffer_size.as_deref(),
                args.base.buffered_copy_threshold.as_deref(),
            )?;

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
        None => Err(CaravanError::InvalidArguments(
            "missing subcommand".to_string(),
        )),
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
        Config::Status {
            state,
            log_level: _,
        } => commands::execute_status(&state),
        Config::Resume {
            state,
            log_level: _,
        } => commands::execute_resume(&state),
    }
}

pub fn execute_transfer(config: TransferConfig) -> Result<(), CaravanError> {
    commands::execute_transfer(config)
}
