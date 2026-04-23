use std::fs;

#[test]
fn prompt_module_no_longer_exposes_conflict_skip_prompt_api() {
    let content = fs::read_to_string("src/prompt.rs").expect("read prompt module");
    assert!(
        !content.contains("confirm_conflict_skip"),
        "prompt module should not expose conflict-skip prompt API anymore"
    );
    assert!(
        !content.contains("ConflictReport"),
        "prompt module should not depend on conflict report types"
    );
}
