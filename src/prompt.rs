use crate::error::WololoError;

pub trait PromptBackend {
    fn confirm_deletion(&self, batch_id: &str) -> Result<bool, WololoError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InteractivePrompt;

impl PromptBackend for InteractivePrompt {
    fn confirm_deletion(&self, _batch_id: &str) -> Result<bool, WololoError> {
        Err(WololoError::NotImplemented(
            "interactive prompt backend is not wired yet",
        ))
    }
}

pub fn request_approval(
    backend: Option<&dyn PromptBackend>,
    interactive: bool,
    explicit_approval: bool,
    batch_id: &str,
) -> Result<bool, WololoError> {
    if explicit_approval {
        return Ok(true);
    }

    if interactive {
        let prompt = backend.ok_or_else(|| {
            WololoError::InvalidArguments(
                "interactive approval requested but no prompt backend provided".to_string(),
            )
        })?;
        return prompt.confirm_deletion(batch_id);
    }

    Err(WololoError::InvalidArguments(
        "destructive operations are blocked in non-interactive mode without explicit approval"
            .to_string(),
    ))
}
