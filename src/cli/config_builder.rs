use crate::config::{Config, Mode, TransferConfig};
use crate::error::CaravanError;

use super::{args, Cli, Command, TransferArgs};

pub(super) fn to_config(cli: Cli) -> Result<Config, CaravanError> {
    let Cli { log_level, command } = cli;

    match command {
        Some(Command::Staging(args)) => {
            validate_transfer_args(&args)?;
            if args.max_files == Some(0) {
                return Err(CaravanError::InvalidArguments(
                    "max-files must be greater than zero".to_string(),
                ));
            }

            let transfer = build_transfer_config(Mode::Staging, args, None, log_level)?;
            Ok(Config::Staging(transfer))
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

            let transfer =
                build_transfer_config(Mode::Migrate, args.base, args.snapshot_every, log_level)?;
            Ok(Config::Migrate(transfer))
        }
        Some(Command::Status(args)) => Ok(Config::Status {
            state: args.state,
            log_level,
        }),
        Some(Command::Resume(args)) => Ok(Config::Resume {
            state: args.state,
            log_level,
            recover_failed: args.recover_failed,
            inspect_failed: args.inspect_failed,
        }),
        None => Err(CaravanError::InvalidArguments(
            "missing subcommand".to_string(),
        )),
    }
}

fn build_transfer_config(
    mode: Mode,
    args: TransferArgs,
    snapshot_every: Option<u32>,
    log_level: String,
) -> Result<TransferConfig, CaravanError> {
    let (copy_buffer_size, buffered_copy_threshold) = args::parse_copy_options(
        args.copy_buffer_size.as_deref(),
        args.buffered_copy_threshold.as_deref(),
    )?;

    Ok(TransferConfig {
        mode,
        source: args.source,
        dest: args.dest,
        batch_size_bytes: args.batch_size,
        max_files: args.max_files,
        snapshot_every,
        interactive: args.interactive,
        verification: args.verification.into(),
        log_level,
        skip_conflicts: args.skip_conflicts,
        recover_failed: args.recover_failed,
        copy_strategy: args.copy_strategy.into(),
        copy_buffer_size,
        buffered_copy_threshold,
    })
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
