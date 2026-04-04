use crate::error::CaravanError;

pub trait PromptBackend {
    fn confirm_deletion(&self, batch_id: &str) -> Result<bool, CaravanError>;
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