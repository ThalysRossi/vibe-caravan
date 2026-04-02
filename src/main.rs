use std::process::ExitCode;

use wololo::cli;

fn main() -> ExitCode {
    if std::env::args().len() == 1 {
        eprintln!("No arguments provided. Use --help for usage.");
        return ExitCode::from(2);
    }

    match cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}
