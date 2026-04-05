use std::time::{Instant, Duration};

/// Progress reporter trait for tracking long running operations
pub trait ProgressReporter {
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
    fn start(&mut self, _total_items: usize, _operation: &str) {}
    fn advance(&mut self, _current: usize, _item_name: Option<&str>) {}
    fn finish(&mut self) {}
}

/// Terminal progress bar implementation
pub struct TerminalProgress {
    total: usize,
    start_time: Instant,
    operation: String,
    last_printed: usize,
}

impl TerminalProgress {
    pub fn new() -> Self {
        TerminalProgress {
            total: 0,
            start_time: Instant::now(),
            operation: String::new(),
            last_printed: 0,
        }
    }
    
    fn format_duration(d: Duration) -> String {
        let secs = d.as_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else {
            format!("{}m {}s", secs / 60, secs % 60)
        }
    }
}

impl ProgressReporter for TerminalProgress {
    fn start(&mut self, total_items: usize, operation: &str) {
        self.total = total_items;
        self.start_time = Instant::now();
        self.operation = operation.to_string();
        self.last_printed = 0;
        eprint!("\r  {} 0/{} files", self.operation, self.total);
    }
    
    fn advance(&mut self, current: usize, _item_name: Option<&str>) {
        // Only update every 10 files or at completion to reduce terminal spam
        if current - self.last_printed >= 10 || current == self.total {
            self.last_printed = current;
            
            let percent = if self.total > 0 {
                (current as f64 / self.total as f64) * 100.0
            } else {
                0.0
            };
            
            let elapsed = self.start_time.elapsed();
            let eta = if current > 0 {
                let remaining = (elapsed / current as u32) * (self.total - current) as u32;
                format!("ETA {}", Self::format_duration(remaining))
            } else {
                String::from("ETA --")
            };
            
            eprint!("\r  {} {}/{} files | {:.1}% | {}", 
                self.operation, current, self.total, percent, eta);
        }
    }
    
    fn finish(&mut self) {
        let elapsed = self.start_time.elapsed();
        eprintln!("\r  {} {}/{} files | Done in {}", 
            self.operation, self.total, self.total, Self::format_duration(elapsed));
    }
}