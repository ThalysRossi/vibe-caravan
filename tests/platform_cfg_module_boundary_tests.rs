use std::fs;

const MODULE_ROOTS: [&str; 3] = [
    "src/scan/mod.rs",
    "src/preflight/mod.rs",
    "src/capacity/mod.rs",
];

fn collect_cfg_boundary_violations(path: &str, content: &str) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut violations = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("#[cfg(") {
            continue;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            violations.push(format!(
                "{path}:{} cfg attribute must be at module scope (found indentation)",
                idx + 1
            ));
        }

        let mut next_idx = idx + 1;
        while next_idx < lines.len() {
            let next_trimmed = lines[next_idx].trim_start();
            if next_trimmed.is_empty()
                || next_trimmed.starts_with("//")
                || next_trimmed.starts_with("#[")
            {
                next_idx += 1;
                continue;
            }
            break;
        }

        if next_idx >= lines.len() {
            violations.push(format!(
                "{path}:{} cfg attribute has no declaration after it",
                idx + 1
            ));
            continue;
        }

        let next = lines[next_idx].trim_start();
        let allowed_boundary_declaration = next.starts_with("mod ")
            || next.starts_with("pub mod ")
            || next.starts_with("use ")
            || next.starts_with("pub use ")
            || next.starts_with("pub(crate) use ")
            || next.starts_with("pub(super) use ");
        if !allowed_boundary_declaration {
            violations.push(format!(
                "{path}:{} cfg attribute must guard module-boundary mod/use declaration, found `{}`",
                idx + 1,
                next
            ));
        }
    }

    violations
}

#[test]
fn module_root_cfg_attributes_are_only_module_boundary_declarations() {
    let mut violations = Vec::new();

    for path in MODULE_ROOTS {
        let content = fs::read_to_string(path).expect("failed to read module root");
        let mut file_violations = collect_cfg_boundary_violations(path, &content);
        violations.append(&mut file_violations);
    }

    assert!(
        violations.is_empty(),
        "found cfg boundary violations in module roots:\n{}",
        violations.join("\n")
    );
}

#[test]
fn boundary_validator_flags_function_level_cfg_and_allows_boundary_cfg() {
    let content = r#"
#[cfg(target_os = "windows")]
mod windows;

    #[cfg(target_os = "linux")]
    fn helper() {}
"#;
    let violations = collect_cfg_boundary_violations("synthetic.rs", content);
    assert!(
        violations
            .iter()
            .any(|item| item.contains("must be at module scope")),
        "expected validator to flag indented cfg attribute, got: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|item| item.contains("module-boundary")),
        "expected validator to flag function-level cfg declaration, got: {violations:?}"
    );
}
