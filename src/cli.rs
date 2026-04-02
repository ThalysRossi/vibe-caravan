use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::config::{Config, Mode, TransferConfig, VerificationMode};
use crate::error::WololoError;

#[derive(Debug, Parser)]
#[command(name = "wololo", version, about = "Safe staged data migration tool")]
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
    #[arg(long, default_value = ".wololo/state.json")]
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

pub fn parse_cli_from<I, T>(args: I) -> Result<Config, WololoError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).map_err(|err| WololoError::Cli(err.to_string()))?;
    to_config(cli)
}

fn to_config(cli: Cli) -> Result<Config, WololoError> {
    match cli.command {
        Some(Command::Staging(args)) => {
            validate_transfer_args(&args)?;
            if args.max_files == Some(0) {
                return Err(WololoError::InvalidArguments(
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
                return Err(WololoError::InvalidArguments(
                    "snapshot-every must be greater than zero when provided".to_string(),
                ));
            }
            if args.base.max_files == Some(0) {
                return Err(WololoError::InvalidArguments(
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
        None => Err(WololoError::InvalidArguments("missing subcommand".to_string())),
    }
}

fn validate_transfer_args(args: &TransferArgs) -> Result<(), WololoError> {
    if args.batch_size == 0 {
        return Err(WololoError::InvalidArguments(
            "batch-size must be greater than zero".to_string(),
        ));
    }
    if args.source == args.dest {
        return Err(WololoError::InvalidArguments(
            "source and destination must differ".to_string(),
        ));
    }
    Ok(())
}

pub fn run() -> Result<(), WololoError> {
    let config = parse_cli_from(std::env::args())?;

    match config {
        Config::Staging(_) | Config::Migrate(_) | Config::Status { .. } | Config::Resume { .. } => {
            Err(WololoError::NotImplemented("phase 2 parsing only"))
        }
    }
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
