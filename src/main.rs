use std::io::{self, BufRead, IsTerminal, Write};
use std::process::ExitCode;
use std::time::Duration;

use caravan::cli;

fn main() -> ExitCode {
    if std::env::args().len() == 1 {
        eprintln!("No arguments provided. Use --help for usage.");
        pause_on_error_if_needed();
        return ExitCode::from(2);
    }

    match cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            pause_on_error_if_needed();
            ExitCode::from(1)
        }
    }
}

fn pause_on_error_if_needed() {
    if !should_pause_on_error() {
        return;
    }

    if pause_via_controlling_terminal() {
        return;
    }

    if pause_via_stdio() {
        return;
    }

    if should_delay_exit_when_input_unavailable() {
        eprintln!();
        eprintln!("No interactive input channel detected; exiting in 10 seconds...");
        std::thread::sleep(Duration::from_secs(10));
    }
}

fn should_pause_on_error() -> bool {
    if env_flag_enabled("CARAVAN_DISABLE_PAUSE_ON_ERROR") {
        return false;
    }

    if env_flag_enabled("CARAVAN_FORCE_PAUSE_ON_ERROR") {
        return true;
    }

    if interactive_flag_present_in_raw_args() {
        return true;
    }

    io::stdin().is_terminal() || io::stderr().is_terminal() || has_controlling_terminal()
}

fn env_flag_enabled(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn pause_via_stdio() -> bool {
    eprintln!();
    eprint!("Press Enter to exit...");
    let _ = io::stderr().flush();

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(read) => read > 0,
        Err(_) => false,
    }
}

fn should_delay_exit_when_input_unavailable() -> bool {
    env_flag_enabled("CARAVAN_FORCE_PAUSE_ON_ERROR") || interactive_flag_present_in_raw_args()
}

fn interactive_flag_present_in_raw_args() -> bool {
    std::env::args_os().any(|arg| arg == "--interactive")
}

#[cfg(target_os = "linux")]
fn has_controlling_terminal() -> bool {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .is_ok()
}

#[cfg(target_os = "windows")]
fn has_controlling_terminal() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn pause_via_controlling_terminal() -> bool {
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    {
        Ok(file) => file,
        Err(_) => return false,
    };

    let mut writer = match file.try_clone() {
        Ok(writer) => writer,
        Err(_) => return false,
    };

    let _ = writeln!(writer);
    let _ = write!(writer, "Press Enter to exit...");
    let _ = writer.flush();

    let mut input = String::new();
    let mut reader = io::BufReader::new(file);
    let _ = reader.read_line(&mut input);
    true
}

#[cfg(target_os = "windows")]
fn pause_via_controlling_terminal() -> bool {
    false
}
