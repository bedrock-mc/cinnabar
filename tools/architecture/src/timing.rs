use std::path::{Path, PathBuf};

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
        let rust_test = rust && is_rust_test(&relative);
        let clock_free = rust
            && rules
                .clock_free_crates
                .iter()
                .any(|krate| relative.starts_with(&format!("{krate}/src/")));
        let source = read(path)?;
        let exception = rules
            .sleep_exceptions
            .iter()
            .position(|exception| exception.path == relative);
        let mut scopes = TestScopes::default();
        for (index, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            let in_test = rust_test || (rust && scopes.advance(code));
            let sleeps = (go_test && code.contains("time.Sleep("))
                || (in_test && code.contains("thread::sleep("));
            if sleeps {
                match exception {
                    Some(position) => used[position] = true,
                    None => diagnostics.push(format!("{relative}:{}: {SLEEP_HELP}", index + 1)),
                }
            }
            if clock_free
                && !in_test
                && (code.contains("Instant::now()") || code.contains("SystemTime::now()"))
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

/// Test files and test-only directories, including `foo_tests/` helper modules, by repository-relative path.
fn is_rust_test(relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or("");
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || relative
            .split('/')
            .rev()
            .skip(1)
            .any(|dir| dir == "tests" || dir.ends_with("_tests"))
}

/// Tracks items under a `#[cfg(...test...)]` attribute line by line, by brace depth.
#[derive(Default)]
struct TestScopes {
    pending: bool,
    depth: usize,
}

impl TestScopes {
    /// Returns whether `code` lies inside a test-only item, then updates the scope.
    fn advance(&mut self, code: &str) -> bool {
        let trimmed = code.trim_start();
        if self.depth == 0 && !self.pending {
            self.pending = is_test_cfg(trimmed);
            return self.pending;
        }
        if self.depth == 0 && (trimmed.starts_with("#[") || trimmed.is_empty()) {
            return true;
        }
        let was_open = self.depth > 0;
        for byte in code.bytes() {
            match byte {
                b'{' => self.depth += 1,
                b'}' => self.depth = self.depth.saturating_sub(1),
                _ => {}
            }
        }
        if self.depth == 0 && (was_open || code.contains(['{', '}', ';', ','])) {
            self.pending = false;
        }
        true
    }
}

fn is_test_cfg(attribute: &str) -> bool {
    attribute.starts_with("#[cfg(")
        && !attribute.contains("not(test")
        && attribute
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|word| word == "test")
}

#[cfg(test)]
mod tests {
    use super::{TestScopes, is_rust_test};

    #[test]
    fn test_files_and_helper_directories_count_as_tests() {
        for path in [
            "crates/a/src/tests.rs",
            "crates/a/src/foo_tests.rs",
            "crates/a/tests/it/main.rs",
            "crates/a/src/foo_tests/carrier.rs",
        ] {
            assert!(is_rust_test(path), "{path}");
        }
        assert!(!is_rust_test("crates/a/src/contests.rs"));
        assert!(!is_rust_test("crates/a/src/latest/mod.rs"));
    }

    fn test_lines(source: &str) -> Vec<usize> {
        let mut scopes = TestScopes::default();
        source
            .lines()
            .enumerate()
            .filter(|(_, line)| scopes.advance(line))
            .map(|(index, _)| index + 1)
            .collect()
    }

    // Inline test modules and cfg(test) helpers count as tests; code after them does not.
    #[test]
    fn cfg_test_items_are_test_scopes() {
        let source = "fn live() {}\n#[cfg(all(test, unix))]\nmod tests {\n    fn a() {\n        sleep();\n    }\n}\nfn after() {}\n#[cfg(test)]\nfn helper() -> u32 {\n    1\n}\n#[cfg(not(test))]\nfn release() {}\n#[cfg(test)]\nmod external;\nfn last() {}";
        assert_eq!(test_lines(source), vec![2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 15, 16]);
    }
}
