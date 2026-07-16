use std::path::{Component, Path, PathBuf};

use crate::{ArchitectureError, paths::relative_slash, policy::Policy, read};

pub(super) fn check_artifacts(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) {
    for path in files {
        let relative = relative_slash(root, path);
        for pattern in &policy.forbidden_artifacts {
            if wildcard_match(pattern.as_bytes(), relative.as_bytes()) {
                diagnostics.push(format!(
                    "{relative}: matches forbidden artifact pattern `{pattern}`"
                ));
            }
        }
    }
}

fn wildcard_match(pattern: &[u8], value: &[u8]) -> bool {
    let (mut pattern_index, mut value_index) = (0, 0);
    let (mut star_index, mut star_value_index) = (None, 0);
    while value_index < value.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == value[value_index] {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
                pattern_index += 1;
            }
            star_index = Some(pattern_index);
            star_value_index = value_index;
        } else if let Some(after_star) = star_index {
            star_value_index += 1;
            value_index = star_value_index;
            pattern_index = after_star;
        } else {
            return false;
        }
    }
    pattern[pattern_index..].iter().all(|byte| *byte == b'*')
}

pub(super) fn check_sources(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    for path in files {
        let extension = path.extension().and_then(|value| value.to_str());
        if !matches!(extension, Some("rs" | "ps1")) {
            continue;
        }
        let relative = relative_slash(root, path);
        let source = read(path)?;
        let lines = source.lines().count();
        if extension == Some("ps1") {
            if lines > policy.powershell_max {
                diagnostics.push(format!(
                    "{relative}: {lines} lines exceeds PowerShell limit {}",
                    policy.powershell_max
                ));
            }
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let is_test = path
            .components()
            .any(|part| matches!(part, Component::Normal(value) if value == "tests"));
        let (limit, label) = if is_test {
            (policy.test_max, "test")
        } else if matches!(file_name, "lib.rs" | "main.rs" | "mod.rs") {
            (policy.module_root_max, "module-root")
        } else {
            (policy.production_rust_max, "production Rust")
        };
        if lines > limit {
            diagnostics.push(format!(
                "{relative}: {lines} lines exceeds {label} limit {limit}"
            ));
        }
        for (index, line) in source.lines().enumerate() {
            let compact = line.split_whitespace().collect::<String>();
            if compact.starts_with("pubuse") && compact.contains("::*") {
                diagnostics.push(format!(
                    "{relative}:{}: glob re-export is forbidden",
                    index + 1
                ));
            }
            if line.contains("pub fn ") && line.contains("_for_test") {
                diagnostics.push(format!(
                    "{relative}:{}: test-only public API is forbidden",
                    index + 1
                ));
            }
        }
    }
    Ok(())
}
