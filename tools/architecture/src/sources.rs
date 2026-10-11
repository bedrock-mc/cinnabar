use std::path::{Component, Path, PathBuf};

use workspace_declarations::wildcard_match;

use crate::{ArchitectureError, paths::relative_slash, policy::Policy, read};

pub(super) fn check_artifacts(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) {
    for path in files {
        let relative = relative_slash(root, path);
        if policy
            .owned_artifacts
            .iter()
            .any(|owned| relative.starts_with(&owned.path))
        {
            continue;
        }
        for pattern in &policy.forbidden_artifacts {
            if wildcard_match(pattern, &relative) {
                diagnostics.push(format!(
                    "{relative}: matches forbidden artifact pattern `{pattern}`"
                ));
            }
        }
    }
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
            .any(|part| matches!(part, Component::Normal(value) if value == "tests"))
            || file_name == "tests.rs"
            || file_name.ends_with("_tests.rs");
        let (limit, label) = if is_test {
            (policy.test_max, "test")
        } else if matches!(file_name, "lib.rs" | "main.rs" | "mod.rs") {
            (policy.module_root_max, "module-root")
        } else {
            (policy.production_rust_max, "production Rust")
        };
        if lines > limit {
            let baseline = policy
                .line_baselines
                .iter()
                .find(|baseline| baseline.path == relative)
                .map(|baseline| baseline.max)
                .filter(|baseline| *baseline > limit);
            match baseline {
                Some(baseline) if lines > baseline => diagnostics.push(format!(
                    "{relative}: {lines} lines exceeds grandfathered baseline {baseline}"
                )),
                Some(_) => {}
                None => diagnostics.push(format!(
                    "{relative}: {lines} lines exceeds {label} limit {limit}"
                )),
            }
        }
        for (index, line) in source.lines().enumerate() {
            let compact = line.split_whitespace().collect::<String>();
            // Build-script output in `OUT_DIR` is generated code, not a first-party fragment.
            let generated = compact.starts_with("include!(concat!(env!(\"OUT_DIR\")");
            if !is_test && compact.starts_with("include!(") && !generated {
                diagnostics.push(format!(
                    "{relative}:{}: first-party Rust source fragments are forbidden",
                    index + 1
                ));
            }
            if compact == "usecrate::*;" {
                diagnostics.push(format!(
                    "{relative}:{}: crate-wide private preludes are forbidden",
                    index + 1
                ));
            }
            let test_only_public = compact.starts_with("pub")
                && compact.contains("fn")
                && (compact.contains("_for_test")
                    || compact.contains("fnfor_test")
                    || compact.contains("fntest_"));
            if test_only_public {
                diagnostics.push(format!(
                    "{relative}:{}: test-only public API is forbidden",
                    index + 1
                ));
            }
        }
    }
    Ok(())
}
