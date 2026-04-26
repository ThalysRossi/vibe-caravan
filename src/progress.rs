use std::time::{Duration, Instant};
use std::{io::Write, io::stderr};

/// Progress reporter trait for tracking long running operations
pub trait ProgressReporter {
    /// Optional total-byte hint for throughput reporting.
    fn set_total_bytes(&mut self, _total_bytes: u64) {}

    /// Called when operation starts with total number of items
    fn start(&mut self, total_items: usize, operation: &str);

    /// Called after each item is processed
    fn advance(&mut self, current: usize, item_name: Option<&str>);

    /// Called when operation completes successfully
    fn finish(&mut self);
}

/// No-op progress reporter that does nothing
#[derive(Debug, Default)]
pub struct NoopProgress;

impl ProgressReporter for NoopProgress {
    fn set_total_bytes(&mut self, _total_bytes: u64) {}
    fn start(&mut self, _total_items: usize, _operation: &str) {}
    fn advance(&mut self, _current: usize, _item_name: Option<&str>) {}
    fn finish(&mut self) {}
}

/// Terminal progress bar implementation
pub struct TerminalProgress {
    total: usize,
    total_bytes: Option<u64>,
    start_time: Instant,
    operation: String,
    last_printed_percent: f64,
    last_printed_time: Instant,
}

impl TerminalProgress {
    pub fn new() -> Self {
        TerminalProgress {
            total: 0,
            total_bytes: None,
            start_time: Instant::now(),
            operation: String::new(),
            last_printed_percent: -1.0, // Ensure first update
            last_printed_time: Instant::now(),
        }
    }

    /// Set total bytes for the operation (for MB/s calculation)
    pub fn set_total_bytes(&mut self, total_bytes: u64) {
        self.total_bytes = Some(total_bytes);
    }

    fn format_duration(d: Duration) -> String {
        let secs = d.as_secs();
        match secs {
            0..=59 => format!("{}s", secs),
            60..=3599 => {
                let mins = secs / 60;
                let secs_remain = secs % 60;
                format!("{}m {:02}s", mins, secs_remain)
            }
            _ => {
                let hours = secs / 3600;
                let mins = (secs % 3600) / 60;
                let secs_remain = secs % 60;
                format!("{}h {:02}m {:02}s", hours, mins, secs_remain)
            }
        }
    }

    fn progress_bar(percent: f64, width: usize) -> String {
        let filled = (percent * width as f64 / 100.0).round() as usize;
        let filled = filled.min(width);
        let empty = width - filled;
        format!("[{}{}]", "=".repeat(filled), " ".repeat(empty))
    }

    /// Calculate ETA string based on current progress and elapsed time
    fn calculate_eta(&self, current: usize, elapsed: Duration) -> String {
        match (current > 0, current < self.total) {
            (true, true) => {
                let elapsed_secs = elapsed.as_secs_f64();
                let secs_per_item = elapsed_secs / current as f64;
                let remaining_items = (self.total - current) as f64;
                let remaining_secs = secs_per_item * remaining_items;
                let remaining = Duration::from_secs_f64(remaining_secs);
                format!("ETA {}", Self::format_duration(remaining))
            }
            _ => String::from("ETA --"),
        }
    }

    /// Calculate throughput string based on elapsed time and total bytes
    fn calculate_throughput(&self, current: usize, elapsed: Duration) -> String {
        match (elapsed.as_secs() > 0, self.total_bytes) {
            (true, Some(total_bytes)) => {
                let files_per_sec = current as f64 / elapsed.as_secs_f64();
                let estimated_bytes_copied =
                    (current as f64 / self.total as f64) * total_bytes as f64;
                let mb_per_sec = estimated_bytes_copied / elapsed.as_secs_f64() / (1024.0 * 1024.0);
                format!("{:.1} files/s, {:.1} MB/s", files_per_sec, mb_per_sec)
            }
            (true, None) => {
                let files_per_sec = current as f64 / elapsed.as_secs_f64();
                format!("{:.1} files/s", files_per_sec)
            }
            (false, Some(_)) => String::from("-- files/s, -- MB/s"),
            (false, None) => String::from("-- files/s"),
        }
    }

    /// Determine if the progress display should be updated
    fn should_update_display(&self, current: usize, percent: f64) -> bool {
        current == self.total
            || (percent - self.last_printed_percent).abs() >= 0.5
            || self.last_printed_time.elapsed().as_millis() >= 100
    }
}

impl Default for TerminalProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressReporter for TerminalProgress {
    fn set_total_bytes(&mut self, total_bytes: u64) {
        self.total_bytes = Some(total_bytes);
    }

    fn start(&mut self, total_items: usize, operation: &str) {
        self.total = total_items;
        self.start_time = Instant::now();
        self.operation = operation.to_string();
        self.last_printed_percent = -1.0;
        self.last_printed_time = Instant::now();
        let bar = Self::progress_bar(0.0, 20);
        let throughput = if self.total_bytes.is_some() {
            "-- files/s, -- MB/s"
        } else {
            "-- files/s"
        };
        eprint!(
            "\r  {} 0/{} files {} 0.0% | {} | ETA --",
            self.operation, self.total, bar, throughput
        );
        let _ = stderr().flush();
    }

    fn advance(&mut self, current: usize, _item_name: Option<&str>) {
        // Calculate current percentage
        let percent = if self.total > 0 {
            (current as f64 / self.total as f64) * 100.0
        } else {
            0.0
        };

        // Use helper method to determine if we should update
        if !self.should_update_display(current, percent) {
            return;
        }

        self.last_printed_percent = percent;
        self.last_printed_time = Instant::now();

        let elapsed = self.start_time.elapsed();

        // Use helper methods for calculations
        let eta = self.calculate_eta(current, elapsed);
        let throughput = self.calculate_throughput(current, elapsed);
        let bar = Self::progress_bar(percent, 20);

        eprint!(
            "\r  {} {}/{} files {} {:.1}% | {} | {}",
            self.operation, current, self.total, bar, percent, throughput, eta
        );
        let _ = stderr().flush();
    }

    fn finish(&mut self) {
        let elapsed = self.start_time.elapsed();
        let bar = Self::progress_bar(100.0, 20);
        eprintln!(
            "\r  {} {}/{} files {} Done in {}",
            self.operation,
            self.total,
            self.total,
            bar,
            Self::format_duration(elapsed)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_handles_seconds_minutes_and_hours() {
        assert_eq!(
            TerminalProgress::format_duration(Duration::from_secs(7)),
            "7s"
        );
        assert_eq!(
            TerminalProgress::format_duration(Duration::from_secs(125)),
            "2m 05s"
        );
        assert_eq!(
            TerminalProgress::format_duration(Duration::from_secs(3723)),
            "1h 02m 03s"
        );
    }

    #[test]
    fn progress_bar_respects_width_and_rounding_bounds() {
        assert_eq!(TerminalProgress::progress_bar(0.0, 5), "[     ]");
        assert_eq!(TerminalProgress::progress_bar(100.0, 5), "[=====]");
        assert_eq!(TerminalProgress::progress_bar(49.0, 10), "[=====     ]");
        assert_eq!(TerminalProgress::progress_bar(200.0, 3), "[===]");
    }

    #[test]
    fn calculate_eta_returns_unknown_without_progress_or_when_complete() {
        let mut progress = TerminalProgress::new();
        progress.total = 10;
        assert_eq!(
            progress.calculate_eta(0, Duration::from_secs(10)),
            "ETA --".to_string()
        );
        assert_eq!(
            progress.calculate_eta(10, Duration::from_secs(10)),
            "ETA --".to_string()
        );
    }

    #[test]
    fn calculate_eta_estimates_remaining_time() {
        let mut progress = TerminalProgress::new();
        progress.total = 10;

        let eta = progress.calculate_eta(5, Duration::from_secs(10));
        assert_eq!(eta, "ETA 10s");
    }

    #[test]
    fn calculate_throughput_reports_files_and_optional_mb_rate() {
        let mut progress = TerminalProgress::new();
        progress.total = 10;
        progress.total_bytes = Some(10 * 1024 * 1024);

        let throughput = progress.calculate_throughput(5, Duration::from_secs(2));
        assert_eq!(throughput, "2.5 files/s, 2.5 MB/s");

        progress.total_bytes = None;
        let throughput_no_bytes = progress.calculate_throughput(5, Duration::from_secs(2));
        assert_eq!(throughput_no_bytes, "2.5 files/s");
    }

    #[test]
    fn calculate_throughput_reports_placeholders_before_one_second() {
        let mut progress = TerminalProgress::new();
        progress.total = 10;
        progress.total_bytes = Some(10 * 1024 * 1024);
        assert_eq!(
            progress.calculate_throughput(1, Duration::from_millis(100)),
            "-- files/s, -- MB/s"
        );

        progress.total_bytes = None;
        assert_eq!(
            progress.calculate_throughput(1, Duration::from_millis(100)),
            "-- files/s"
        );
    }

    #[test]
    fn should_update_display_triggers_for_completion_percentage_delta_or_time() {
        let mut progress = TerminalProgress::new();
        progress.total = 10;
        progress.last_printed_percent = 10.0;
        progress.last_printed_time = Instant::now();

        assert!(!progress.should_update_display(2, 10.1));
        assert!(progress.should_update_display(10, 10.1));
        assert!(progress.should_update_display(2, 10.6));

        progress.last_printed_time = Instant::now() - Duration::from_millis(150);
        assert!(progress.should_update_display(2, 10.1));
    }
}
