use crate::error::CaravanError;
use crate::format::format_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchSizeMismatchChoice {
    UseStateSize,
    EnterNewSize,
    StartFresh,
}

fn parse_batch_size_mismatch_choice(choice: &str) -> Result<BatchSizeMismatchChoice, CaravanError> {
    match choice {
        "1" => Ok(BatchSizeMismatchChoice::UseStateSize),
        "2" => Ok(BatchSizeMismatchChoice::EnterNewSize),
        "3" => Ok(BatchSizeMismatchChoice::StartFresh),
        _ => Err(CaravanError::InvalidArguments(format!(
            "Invalid choice '{}'. Please enter 1, 2, or 3.",
            choice
        ))),
    }
}

fn parse_yes_no(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

fn should_confirm_single_batch(batch_ids: &[String]) -> bool {
    batch_ids.len() == 1
}

pub trait PromptBackend {
    fn confirm_deletion(&self, batch_id: &str) -> Result<bool, CaravanError>;
    fn confirm_batch_deletion(&self, batch_ids: &[String]) -> Result<bool, CaravanError> {
        for batch_id in batch_ids {
            if !self.confirm_deletion(batch_id)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn ask_batch_size_mismatch(
        &self,
        state_size: u64,
        cli_size: u64,
    ) -> Result<BatchSizeMismatchChoice, CaravanError> {
        use std::io::{self, Write};

        println!("\n⚠️  Batch size mismatch detected!");
        println!("   State file batch size: {}", format_bytes(state_size));
        println!("   CLI argument batch size: {}", format_bytes(cli_size));
        println!("\nPlease choose an option:");
        println!(
            "  1) Use batch size from state file ({})",
            format_bytes(state_size)
        );
        println!("  2) Enter new batch size");
        println!("  3) Start fresh migration (overwrites state file)");
        print!("\nEnter choice (1-3): ");
        io::stdout()
            .flush()
            .map_err(|e| CaravanError::Io(format!("failed to flush stdout: {}", e)))?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| CaravanError::Io(format!("failed to read user input: {}", e)))?;

        parse_batch_size_mismatch_choice(input.trim())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InteractivePrompt;

impl PromptBackend for InteractivePrompt {
    fn confirm_deletion(&self, batch_id: &str) -> Result<bool, CaravanError> {
        use std::io::{self, Write};

        print!(
            "Approve deletion of source files for batch '{}'? (y/n): ",
            batch_id
        );
        io::stdout()
            .flush()
            .map_err(|e| CaravanError::Io(format!("failed to flush stdout: {}", e)))?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| CaravanError::Io(format!("failed to read user input: {}", e)))?;

        Ok(parse_yes_no(&input))
    }

    fn confirm_batch_deletion(&self, batch_ids: &[String]) -> Result<bool, CaravanError> {
        use std::io::{self, Write};

        if batch_ids.is_empty() {
            return Ok(true);
        }

        if should_confirm_single_batch(batch_ids) {
            return self.confirm_deletion(&batch_ids[0]);
        }

        println!(
            "All {} batches have been verified successfully.",
            batch_ids.len()
        );
        print!("Approve deletion of source files for ALL batches? (y/n): ");
        io::stdout()
            .flush()
            .map_err(|e| CaravanError::Io(format!("failed to flush stdout: {}", e)))?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| CaravanError::Io(format!("failed to read user input: {}", e)))?;

        Ok(parse_yes_no(&input))
    }
}

pub fn request_approval(
    backend: Option<&dyn PromptBackend>,
    interactive: bool,
    explicit_approval: bool,
    batch_id: &str,
) -> Result<bool, CaravanError> {
    if explicit_approval {
        return Ok(true);
    }

    if interactive {
        let prompt = backend.ok_or_else(|| {
            CaravanError::InvalidArguments(
                "interactive approval requested but no prompt backend provided".to_string(),
            )
        })?;
        return prompt.confirm_deletion(batch_id);
    }

    Err(CaravanError::InvalidArguments(
        "destructive operations are blocked in non-interactive mode without explicit approval"
            .to_string(),
    ))
}

pub fn request_approval_for_batches(
    backend: Option<&dyn PromptBackend>,
    interactive: bool,
    explicit_approval: bool,
    batch_ids: &[String],
) -> Result<bool, CaravanError> {
    if explicit_approval {
        return Ok(true);
    }

    if batch_ids.is_empty() {
        return Ok(true);
    }

    if interactive {
        let prompt = backend.ok_or_else(|| {
            CaravanError::InvalidArguments(
                "interactive approval requested but no prompt backend provided".to_string(),
            )
        })?;
        return prompt.confirm_batch_deletion(batch_ids);
    }

    Err(CaravanError::InvalidArguments(
        "destructive operations are blocked in non-interactive mode without explicit approval"
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;

    #[derive(Debug, Clone, Copy)]
    enum ConfirmStep {
        Allow(bool),
        Fail(&'static str),
    }

    struct SequencePrompt {
        steps: RefCell<Vec<ConfirmStep>>,
        calls: RefCell<Vec<String>>,
    }

    impl SequencePrompt {
        fn new(steps: Vec<ConfirmStep>) -> Self {
            Self {
                steps: RefCell::new(steps),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl PromptBackend for SequencePrompt {
        fn confirm_deletion(&self, batch_id: &str) -> Result<bool, CaravanError> {
            self.calls.borrow_mut().push(batch_id.to_string());
            let next = self
                .steps
                .borrow_mut()
                .drain(..1)
                .next()
                .expect("missing confirm step");
            match next {
                ConfirmStep::Allow(value) => Ok(value),
                ConfirmStep::Fail(message) => Err(CaravanError::Io(message.to_string())),
            }
        }
    }

    #[test]
    fn default_confirm_batch_deletion_returns_true_when_all_batches_approved() {
        let prompt = SequencePrompt::new(vec![ConfirmStep::Allow(true), ConfirmStep::Allow(true)]);
        let batch_ids = vec!["batch-a".to_string(), "batch-b".to_string()];

        let approved = prompt
            .confirm_batch_deletion(&batch_ids)
            .expect("approval should succeed");
        assert!(approved);
        assert_eq!(prompt.calls.borrow().as_slice(), &["batch-a", "batch-b"]);
    }

    #[test]
    fn default_confirm_batch_deletion_stops_after_first_rejection() {
        let prompt = SequencePrompt::new(vec![ConfirmStep::Allow(false), ConfirmStep::Allow(true)]);
        let batch_ids = vec!["batch-a".to_string(), "batch-b".to_string()];

        let approved = prompt
            .confirm_batch_deletion(&batch_ids)
            .expect("approval should succeed");
        assert!(!approved);
        assert_eq!(prompt.calls.borrow().as_slice(), &["batch-a"]);
    }

    #[test]
    fn default_confirm_batch_deletion_propagates_errors() {
        let prompt = SequencePrompt::new(vec![ConfirmStep::Fail("read failed")]);
        let batch_ids = vec!["batch-a".to_string()];

        let err = prompt
            .confirm_batch_deletion(&batch_ids)
            .expect_err("error must propagate");
        assert!(err.to_string().contains("read failed"));
    }

    #[test]
    fn request_approval_honors_explicit_approval() {
        let approved =
            request_approval(None, false, true, "batch-a").expect("explicit approval should pass");
        assert!(approved);
    }

    #[test]
    fn request_approval_requires_backend_when_interactive() {
        let err = request_approval(None, true, false, "batch-a")
            .expect_err("interactive mode without backend must fail");
        assert!(err.to_string().contains("no prompt backend provided"));
    }

    #[test]
    fn request_approval_blocks_non_interactive_without_explicit_approval() {
        let err = request_approval(None, false, false, "batch-a")
            .expect_err("non-interactive mode should block");
        assert!(
            err.to_string()
                .contains("destructive operations are blocked")
        );
    }

    #[test]
    fn request_approval_uses_backend_when_interactive() {
        let prompt = SequencePrompt::new(vec![ConfirmStep::Allow(true)]);

        let approved = request_approval(Some(&prompt), true, false, "batch-a")
            .expect("interactive prompt should succeed");
        assert!(approved);
        assert_eq!(prompt.calls.borrow().as_slice(), &["batch-a"]);
    }

    #[test]
    fn request_approval_for_batches_honors_explicit_or_empty_batches() {
        let approved = request_approval_for_batches(None, false, true, &[])
            .expect("explicit approval should pass");
        assert!(approved);

        let approved = request_approval_for_batches(None, false, false, &[])
            .expect("empty batch list should pass");
        assert!(approved);
    }

    #[test]
    fn request_approval_for_batches_requires_backend_in_interactive_mode() {
        let batch_ids = vec!["batch-a".to_string()];
        let err = request_approval_for_batches(None, true, false, &batch_ids)
            .expect_err("interactive mode without backend must fail");
        assert!(err.to_string().contains("no prompt backend provided"));
    }

    #[test]
    fn request_approval_for_batches_blocks_non_interactive_without_explicit_approval() {
        let batch_ids = vec!["batch-a".to_string()];
        let err = request_approval_for_batches(None, false, false, &batch_ids)
            .expect_err("non-interactive mode should block");
        assert!(
            err.to_string()
                .contains("destructive operations are blocked")
        );
    }

    #[test]
    fn request_approval_for_batches_uses_backend_confirmation() {
        let prompt = SequencePrompt::new(vec![ConfirmStep::Allow(true), ConfirmStep::Allow(false)]);
        let batch_ids = vec!["batch-a".to_string(), "batch-b".to_string()];

        let approved = request_approval_for_batches(Some(&prompt), true, false, &batch_ids)
            .expect("interactive prompt should succeed");
        assert!(!approved);
        assert_eq!(prompt.calls.borrow().as_slice(), &["batch-a", "batch-b"]);
    }

    #[test]
    fn parse_batch_size_mismatch_choice_accepts_all_supported_options() {
        assert_eq!(
            parse_batch_size_mismatch_choice("1").expect("option 1 should parse"),
            BatchSizeMismatchChoice::UseStateSize
        );
        assert_eq!(
            parse_batch_size_mismatch_choice("2").expect("option 2 should parse"),
            BatchSizeMismatchChoice::EnterNewSize
        );
        assert_eq!(
            parse_batch_size_mismatch_choice("3").expect("option 3 should parse"),
            BatchSizeMismatchChoice::StartFresh
        );
    }

    #[test]
    fn parse_batch_size_mismatch_choice_rejects_invalid_options() {
        let err = parse_batch_size_mismatch_choice("x").expect_err("invalid choice should fail");
        assert!(err.to_string().contains("Please enter 1, 2, or 3"));
    }

    #[test]
    fn parse_yes_no_only_accepts_yes_answers() {
        assert!(parse_yes_no("y"));
        assert!(parse_yes_no("YES"));
        assert!(!parse_yes_no("n"));
        assert!(!parse_yes_no("no"));
        assert!(!parse_yes_no("maybe"));
    }

    #[test]
    fn should_confirm_single_batch_requires_exactly_one_batch() {
        let zero: Vec<String> = Vec::new();
        let one = vec!["batch-a".to_string()];
        let two = vec!["batch-a".to_string(), "batch-b".to_string()];

        assert!(!should_confirm_single_batch(&zero));
        assert!(should_confirm_single_batch(&one));
        assert!(!should_confirm_single_batch(&two));
    }
}
