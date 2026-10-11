//! Exact stable-libtest failures, tied to their Cargo package and executable target.
use crate::{CommandSpec, DevtoolError};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Serialize, Deserialize, Clone)]
struct Failure {
    manifest: String,
    kind: String,
    target: String,
    test: String,
}

/// Matches each failed test to its compiler artifact in the combined Cargo output stream.
pub(crate) fn parse(text: &str, root: &Path) -> BTreeSet<String> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut artifacts = BTreeMap::new();
    let mut current = None;
    let mut failures = BTreeSet::new();
    let mut current_failed = false;
    let mut current_completed = true;
    let mut completed_failures = 0;
    for line in text.lines() {
        if let Some((executable, artifact)) = test_artifact(line, &root) {
            artifacts.insert(executable, artifact);
        }
        if line.trim_start().starts_with("Running ") {
            if !current_completed {
                return BTreeSet::new();
            }
            current_failed = false;
            current_completed = false;
            current = line
                .rsplit_once('(')
                .and_then(|(_, executable)| executable.strip_suffix(')'))
                .and_then(|executable| {
                    artifacts
                        .get(executable)
                        .or_else(|| {
                            artifacts
                                .iter()
                                .find(|(path, _)| {
                                    Path::new(path).file_name() == Path::new(executable).file_name()
                                })
                                .map(|(_, failure)| failure)
                        })
                        .cloned()
                });
        }
        // Doctests do not expose a stable executable identity. Their gate remains strict.
        if line.trim_start().starts_with("Doc-tests ") {
            if !current_completed {
                return BTreeSet::new();
            }
            current = None;
            current_failed = false;
        }
        if line.starts_with("test result: ") {
            let failed = line.starts_with("test result: FAILED");
            if failed && !current_failed {
                return BTreeSet::new();
            }
            completed_failures += usize::from(failed);
            current_completed = true;
        }
        if let Some(count) = line
            .trim_start()
            .strip_prefix("error: ")
            .and_then(|text| {
                text.strip_suffix(" targets failed:")
                    .or_else(|| text.strip_suffix(" target failed:"))
            })
            .and_then(|count| count.parse::<usize>().ok())
        {
            if !current_completed || count != completed_failures {
                return BTreeSet::new();
            }
            continue;
        }
        if line.trim_start().starts_with("error:")
            && (!line.trim_start().starts_with("error: test failed,")
                || !current_completed
                || !current_failed)
        {
            return BTreeSet::new();
        }
        if let Some(test) = line
            .strip_prefix("test ")
            .and_then(|line| line.strip_suffix(" ... FAILED"))
        {
            if let Some(mut failure) = current.clone() {
                current_failed = true;
                failure.test = test.into();
                failures.insert(serde_json::to_string(&failure).unwrap());
            } else {
                return BTreeSet::new();
            }
        }
    }
    if current_completed {
        failures
    } else {
        BTreeSet::new()
    }
}

/// Reads complete test artifacts owned by this workspace, leaving other Cargo messages alone.
fn test_artifact(line: &str, root: &Path) -> Option<(String, Failure)> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value["reason"] != "compiler-artifact" || value["profile"]["test"] != true {
        return None;
    }
    let executable = value["executable"].as_str()?;
    let path = Path::new(value["manifest_path"].as_str()?);
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let manifest = path.strip_prefix(root).ok()?;
    Some((
        executable.into(),
        Failure {
            manifest: manifest.to_string_lossy().into(),
            kind: value["target"]["kind"]
                .as_array()?
                .first()?
                .as_str()?
                .into(),
            target: value["target"]["name"].as_str()?.into(),
            test: String::new(),
        },
    ))
}

/// Selects exactly one owning package, target, and test on the base worktree.
pub(crate) fn rerun(signature: &str) -> Result<CommandSpec, DevtoolError> {
    let failure: Failure = serde_json::from_str(signature)?;
    let mut args = vec![
        "test".into(),
        "--locked".into(),
        "--manifest-path".into(),
        failure.manifest,
    ];
    match failure.kind.as_str() {
        "lib" | "rlib" | "proc-macro" => args.push("--lib".into()),
        "test" => args.extend(["--test".into(), failure.target]),
        "bin" => args.extend(["--bin".into(), failure.target]),
        "example" => args.extend(["--example".into(), failure.target]),
        _ => {
            return Err(DevtoolError::Usage(
                "unsupported test target; cannot compare base".into(),
            ));
        }
    }
    args.extend(["--".into(), failure.test, "--exact".into()]);
    Ok(CommandSpec::new("cargo", args))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_single_target_summary_preserves_the_failure_identity() {
        use std::{
            fs,
            io::{Read, Seek},
            process::Command,
        };
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[package]\nname='single-target-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n").unwrap();
        fs::write(
            temp.path().join("src/lib.rs"),
            "#[test] fn only_failure() { panic!(\"fixture\"); }",
        )
        .unwrap();
        let mut combined = tempfile::tempfile().unwrap();
        let status = Command::new("cargo")
            .current_dir(temp.path())
            .env("CARGO_TARGET_DIR", temp.path().join("target"))
            .env("CARGO_BUILD_BUILD_DIR", temp.path().join("build"))
            .args([
                "test",
                "--offline",
                "--lib",
                "--no-fail-fast",
                "--message-format=json",
            ])
            .stdout(combined.try_clone().unwrap())
            .stderr(combined.try_clone().unwrap())
            .status()
            .unwrap();
        assert!(!status.success());
        combined.rewind().unwrap();
        let mut text = String::new();
        combined.read_to_string(&mut text).unwrap();
        assert!(
            text.contains("error: 1 target failed:"),
            "unexpected Cargo fixture: {text}"
        );
        let root = temp.path().canonicalize().unwrap();
        let failures = parse(&text, &root);
        assert_eq!(failures.len(), 1, "lost single-target failure: {text}");
        assert!(
            rerun(failures.first().unwrap())
                .unwrap()
                .args
                .contains(&"only_failure".into())
        );
    }

    #[test]
    #[cfg(unix)]
    fn canonical_artifacts_keep_their_identity_through_a_workspace_alias() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        let root = root.canonicalize().unwrap();
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        let output = |manifest: &Path| {
            let artifact = serde_json::json!({"reason":"compiler-artifact", "profile":{"test":true}, "manifest_path":manifest, "executable":"fixture-tests", "target":{"name":"fixture","kind":["lib"]}});
            format!(
                "{artifact}\nRunning unittests src/lib.rs (fixture-tests)\ntest failure ... FAILED\ntest result: FAILED. 0 passed; 1 failed;\nerror: 1 target failed:\n"
            )
        };
        let canonical_output = output(&root.join("Cargo.toml"));
        let expected = parse(&canonical_output, &root);
        assert_eq!(expected.len(), 1);
        assert_eq!(parse(&canonical_output, &alias), expected);
        assert_eq!(parse(&output(&alias.join("Cargo.toml")), &root), expected);
        let command = rerun(expected.first().unwrap()).unwrap();
        assert_eq!(command.args[3], "Cargo.toml");
    }

    #[test]
    fn an_aborting_target_cannot_hide_behind_an_existing_failure() {
        let artifact = |name: &str| {
            serde_json::json!({"reason":"compiler-artifact", "profile":{"test":true}, "manifest_path":"/repo/Cargo.toml", "executable":format!("/repo/target/{name}"), "target":{"name":name,"kind":["test"]}}).to_string()
        };
        let ordinary = format!(
            "{}\nRunning tests/old.rs (/repo/target/old)\ntest existing ... FAILED\ntest result: FAILED. 0 passed; 1 failed;\nerror: test failed, to rerun pass `--test old`\n",
            artifact("old")
        );
        assert_eq!(parse(&ordinary, Path::new("/repo")).len(), 1);
        let mixed = format!(
            "{ordinary}{}\nRunning tests/abort.rs (/repo/target/abort)\nerror: test failed, to rerun pass `--test abort`\nCaused by:\n  process didn't exit successfully: `/repo/target/abort` (signal: 6, SIGABRT: process abort signal)\n",
            artifact("abort")
        );
        assert!(parse(&mixed, Path::new("/repo")).is_empty());
    }

    #[test]
    fn identical_test_names_keep_their_owning_target() {
        let artifact = |name: &str| {
            serde_json::json!({"reason":"compiler-artifact", "profile":{"test":true}, "manifest_path":"/repo/Cargo.toml", "executable":format!("/repo/target/{name}"), "target":{"name":name,"kind":["test"]}}).to_string()
        };
        let text = format!(
            "{}\n{}\nRunning tests/first.rs (/repo/target/first)\ntest same ... FAILED\ntest result: FAILED. 0 passed; 1 failed;\nerror: test failed, to rerun pass `--test first`\nRunning tests/second.rs (/repo/target/second)\ntest same ... FAILED\ntest result: FAILED. 0 passed; 1 failed;\nerror: test failed, to rerun pass `--test second`\nerror: 2 targets failed:\n",
            artifact("first"),
            artifact("second")
        );
        let failures = parse(&text, Path::new("/repo"));
        assert_eq!(failures.len(), 2);
        assert!(
            failures
                .iter()
                .any(|failure| rerun(failure).unwrap().args.contains(&"first".into()))
        );
        assert!(parse("test unknown ... FAILED", Path::new("/repo")).is_empty());
    }
}
