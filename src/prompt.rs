use crate::error::CaravanError;

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
    
    fn ask_batch_size_mismatch(&self, state_size: u64, cli_size: u64) -> Result<crate::detection::BatchSizeMismatchChoice, CaravanError> {
        use std::io::{self, Write};
        
        println!("\n⚠️  Batch size mismatch detected!");
        println!("   State file batch size: {} bytes", state_size);
        println!("   CLI argument batch size: {} bytes", cli_size);
        println!("\nPlease choose an option:");
        println!("  1) Use batch size from state file ({} bytes)", state_size);
        println!("  2) Enter new batch size");
        println!("  3) Start fresh migration (overwrites state file)");
        print!("\nEnter choice (1-3): ");
        io::stdout().flush().map_err(|e| 
            CaravanError::Io(format!("failed to flush stdout: {}", e))
        )?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input).map_err(|e| 
            CaravanError::Io(format!("failed to read user input: {}", e))
        )?;
        
        let choice = input.trim();
        match choice {
            "1" => Ok(crate::detection::BatchSizeMismatchChoice::UseStateSize),
            "2" => Ok(crate::detection::BatchSizeMismatchChoice::EnterNewSize),
            "3" => Ok(crate::detection::BatchSizeMismatchChoice::StartFresh),
            _ => Err(CaravanError::InvalidArguments(
                format!("Invalid choice '{}'. Please enter 1, 2, or 3.", choice)
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InteractivePrompt;

impl PromptBackend for InteractivePrompt {
    fn confirm_deletion(&self, batch_id: &str) -> Result<bool, CaravanError> {
        use std::io::{self, Write};
        
        print!("Approve deletion of source files for batch '{}'? (y/n): ", batch_id);
        io::stdout().flush().map_err(|e| 
            CaravanError::Io(format!("failed to flush stdout: {}", e))
        )?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input).map_err(|e| 
            CaravanError::Io(format!("failed to read user input: {}", e))
        )?;
        
        let answer = input.trim().to_lowercase();
        Ok(matches!(answer.as_str(), "y" | "yes"))
    }

    fn confirm_batch_deletion(&self, batch_ids: &[String]) -> Result<bool, CaravanError> {
        use std::io::{self, Write};
        
        if batch_ids.is_empty() {
            return Ok(true);
        }
        
        if batch_ids.len() == 1 {
            return self.confirm_deletion(&batch_ids[0]);
        }
        
        println!("All {} batches have been verified successfully.", batch_ids.len());
        print!("Approve deletion of source files for ALL batches? (y/n): ");
        io::stdout().flush().map_err(|e| 
            CaravanError::Io(format!("failed to flush stdout: {}", e))
        )?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input).map_err(|e| 
            CaravanError::Io(format!("failed to read user input: {}", e))
        )?;
        
        let answer = input.trim().to_lowercase();
        Ok(matches!(answer.as_str(), "y" | "yes"))
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