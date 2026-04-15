use caravan::error::CaravanError;
use caravan::prompt::{request_approval, PromptBackend};

#[derive(Debug, Clone, Copy)]
struct StubPrompt {
    answer: bool,
}

impl PromptBackend for StubPrompt {
    fn confirm_deletion(&self, _batch_id: &str) -> Result<bool, CaravanError> {
        Ok(self.answer)
    }
    
    fn confirm_batch_deletion(&self, _batch_ids: &[String]) -> Result<bool, CaravanError> {
        // For testing, return the answer for all batches
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

// New tests for batch approval functionality
#[test]
fn batch_approval_prompts_once_for_multiple_batches() {
    use caravan::prompt::request_approval_for_batches;
    
    let backend = StubPrompt { answer: true };
    let batch_ids = vec!["batch-1".to_string(), "batch-2".to_string(), "batch-3".to_string()];
    
    let approved = request_approval_for_batches(Some(&backend), true, false, &batch_ids)
        .expect("batch approval should succeed");
    assert!(approved);
}

#[test]
fn batch_approval_with_explicit_approval_returns_true() {
    use caravan::prompt::request_approval_for_batches;
    
    let batch_ids = vec!["batch-1".to_string()];
    let approved = request_approval_for_batches(None, false, true, &batch_ids)
        .expect("explicit approval should pass");
    assert!(approved);
}

#[test]
fn batch_approval_fails_closed_in_non_interactive_mode() {
    use caravan::prompt::request_approval_for_batches;
    
    let batch_ids = vec!["batch-1".to_string()];
    let err = request_approval_for_batches(None, false, false, &batch_ids)
        .expect_err("destructive action should be blocked");
    assert!(err
        .to_string()
        .contains("destructive operations are blocked"));
}

#[test]
fn empty_batch_list_automatically_approved() {
    use caravan::prompt::request_approval_for_batches;
    
    let batch_ids: Vec<String> = vec![];
    let approved = request_approval_for_batches(None, false, false, &batch_ids)
        .expect("empty batch list should be automatically approved");
    assert!(approved);
}
