use std::path::{Component, Path, PathBuf};

use crate::{ArchitectureError, paths::relative_slash, policy::Policy, read};

const SLEEP_HELP: &str = "tests wait with `test_time::eventually` (Rust) or `testwait` or `testing/synctest` (Go), never sleep";
const CLOCK_HELP: &str = "domain code takes `now` from its caller; only edge code reads the clock";

/// Rejects sleeps in tests and clock reads in clock-free crates' `src/`, except listed exceptions.
pub(super) fn check_timing(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let rules = &policy.timing;
    for exception in &rules.sleep_exceptions {
        if exception.reason.trim().is_empty() {
            diagnostics.push(format!(
                "{}: sleep exception needs a reason",
                exception.path
            ));
        }
    }
    let mut used = vec![false; rules.sleep_exceptions.len()];
    for path in files {
        let relative = relative_slash(root, path);
        let go_test = relative.ends_with("_test.go");
        let rust = relative.ends_with(".rs");
        if !go_test && !rust {
            continue;
        }
        let rust_test = rust && is_rust_test(path);
        let clock_free = rust
            && !rust_test
            && rules
                .clock_free_crates
                .iter()
                .any(|krate| relative.starts_with(&format!("{krate}/src/")));
        if !go_test && !rust_test && !clock_free {
            continue;
        }
        let source = read(path)?;
        let exception = rules
            .sleep_exceptions
            .iter()
            .position(|exception| exception.path == relative);
        for (index, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            let sleeps = (go_test && code.contains("time.Sleep("))
                || (rust_test && code.contains("thread::sleep("));
            if sleeps {
                match exception {
                    Some(position) => used[position] = true,
                    None => diagnostics.push(format!("{relative}:{}: {SLEEP_HELP}", index + 1)),
                }
            }
            if clock_free && (code.contains("Instant::now()") || code.contains("SystemTime::now()"))
            {
                diagnostics.push(format!("{relative}:{}: {CLOCK_HELP}", index + 1));
            }
        }
    }
    for (exception, used) in rules.sleep_exceptions.iter().zip(used) {
        if !used {
            diagnostics.push(format!(
                "{}: stale sleep exception; the file no longer sleeps",
                exception.path
            ));
        }
    }
    Ok(())
}

/// Test files and test-only directories, including `foo_tests/` helper modules.
fn is_rust_test(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || path.components().any(|part| {
            matches!(part, Component::Normal(value)
                if value == "tests" || value.to_str().is_some_and(|dir| dir.ends_with("_tests")))
        })
}

#[cfg(test)]
mod tests {
    use super::is_rust_test;
    use std::path::Path;

    #[test]
    fn test_files_and_helper_directories_count_as_tests() {
        for path in [
            "crates/a/src/tests.rs",
            "crates/a/src/foo_tests.rs",
            "crates/a/tests/it/main.rs",
            "crates/a/src/foo_tests/carrier.rs",
        ] {
            assert!(is_rust_test(Path::new(path)), "{path}");
        }
        assert!(!is_rust_test(Path::new("crates/a/src/contests.rs")));
        assert!(!is_rust_test(Path::new("crates/a/src/latest/mod.rs")));
    }
}
