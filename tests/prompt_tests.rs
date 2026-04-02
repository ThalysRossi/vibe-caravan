use wololo::error::WololoError;
use wololo::prompt::{request_approval, PromptBackend};

#[derive(Debug, Clone, Copy)]
struct StubPrompt {
    answer: bool,
}

impl PromptBackend for StubPrompt {
    fn confirm_deletion(&self, _batch_id: &str) -> Result<bool, WololoError> {
        Ok(self.answer)
    }
}

#[test]
fn explicit_approval_allows_non_interactive_deletion() {
    let approved =
        request_approval(None, false, true, "batch-1").expect("explicit approval should pass");
    assert!(approved);
}

#[test]
fn interactive_prompt_path_uses_backend_answer() {
    let backend = StubPrompt { answer: true };
    let approved = request_approval(Some(&backend), true, false, "batch-1")
        .expect("interactive approval should succeed");
    assert!(approved);
}

#[test]
fn interactive_mode_without_backend_fails_closed() {
    let err = request_approval(None, true, false, "batch-1")
        .expect_err("missing prompt backend should fail");
    assert!(err
        .to_string()
        .contains("interactive approval requested but no prompt backend provided"));
}

#[test]
fn non_interactive_without_explicit_approval_fails_closed() {
    let err = request_approval(None, false, false, "batch-1")
        .expect_err("destructive action should be blocked");
    assert!(err
        .to_string()
        .contains("destructive operations are blocked"));
}
