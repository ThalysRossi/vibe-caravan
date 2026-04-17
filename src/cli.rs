use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::config::{Config, CopyStrategy, OutputFormat, TransferConfig};
use crate::error::CaravanError;
use crate::logging;

mod args;
mod commands;
mod config_builder;

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
    Resume(ResumeArgs),
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
    #[arg(long, default_value_t = false)]
    pub skip_conflicts: bool,
    #[arg(long, default_value_t = false)]
    pub recover_failed: bool,
    #[arg(long, default_value_t = false)]
    pub allow_unsafe_filesystems: bool,
    #[arg(long, value_enum, default_value_t = CopyStrategyArg::Auto)]
    pub copy_strategy: CopyStrategyArg,
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
    #[arg(long)]
    pub snapshot_dir: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
pub struct StateArgs {
    #[arg(long, default_value = ".caravan/state.json")]
    pub state: PathBuf,
    #[arg(long, value_enum, default_value_t = OutputFormatArg::Human)]
    pub output: OutputFormatArg,
}

#[derive(Debug, clap::Args)]
pub struct ResumeArgs {
    #[arg(long, default_value = ".caravan/state.json")]
    pub state: PathBuf,
    #[arg(long, default_value_t = false)]
    pub recover_failed: bool,
    #[arg(long, default_value_t = false)]
    pub inspect_failed: bool,
    #[arg(long, value_enum, default_value_t = OutputFormatArg::Human)]
    pub output: OutputFormatArg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CopyStrategyArg {
    Auto,
    Native,
    Buffered,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormatArg {
    Human,
    Json,
}

impl From<CopyStrategyArg> for CopyStrategy {
    fn from(value: CopyStrategyArg) -> Self {
        match value {
            CopyStrategyArg::Auto => CopyStrategy::Auto,
            CopyStrategyArg::Native => CopyStrategy::Native,
            CopyStrategyArg::Buffered => CopyStrategy::Buffered,
        }
    }
}

impl From<OutputFormatArg> for OutputFormat {
    fn from(value: OutputFormatArg) -> Self {
        match value {
            OutputFormatArg::Human => OutputFormat::Human,
            OutputFormatArg::Json => OutputFormat::Json,
        }
    }
}

pub fn parse_cli_from<I, T>(args: I) -> Result<Config, CaravanError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).map_err(|err| CaravanError::Cli(err.to_string()))?;
    config_builder::to_config(cli)
}

pub fn run() -> Result<(), CaravanError> {
    let config = parse_cli_from(std::env::args())?;
    let log_level = match &config {
        Config::Staging(transfer_config) | Config::Migrate(transfer_config) => {
            transfer_config.log_level.as_str()
        }
        Config::Status { log_level, .. } | Config::Resume { log_level, .. } => log_level.as_str(),
    };
    logging::init_logging(log_level)?;

    match config {
        Config::Staging(transfer_config) | Config::Migrate(transfer_config) => {
            execute_transfer(transfer_config)
        }
        Config::Status {
            state,
            log_level: _,
            output,
        } => commands::execute_status(&state, output),
        Config::Resume {
            state,
            log_level: _,
            recover_failed,
            inspect_failed,
            output,
        } => commands::execute_resume(&state, recover_failed, inspect_failed, output),
    }
}

pub fn execute_transfer(config: TransferConfig) -> Result<(), CaravanError> {
    commands::execute_transfer(config)
}
