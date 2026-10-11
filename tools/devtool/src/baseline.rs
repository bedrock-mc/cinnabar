//! Compares failed checks with the requested base without hiding new failures.
use crate::{CommandSpec, DevtoolError};
use quick_xml::{Reader, events::Event};
mod failures;
use failures::signatures;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

pub(crate) struct Comparison {
    base: String,
    build_dir: String,
    architecture_selector: String,
    root: PathBuf,
    scratch: tempfile::TempDir,
    worktree: Option<PathBuf>,
}

impl Comparison {
    /// Defers creating the detached base worktree until a check fails.
    pub(crate) fn new(base: &str, metadata: &str) -> Result<Self, DevtoolError> {
        let root = output(
            &CommandSpec::new("git", vec!["rev-parse".into(), "--show-toplevel".into()]),
            Path::new("."),
        )?;
        let scratch = tempfile::tempdir().map_err(|source| DevtoolError::Spawn {
            command: "create comparison directory".into(),
            source,
        })?;
        let architecture_selector = crate::commands::package_spec(
            "architecture",
            &crate::packages_from_metadata(metadata)?,
        )
        .to_owned();
        let metadata: serde_json::Value = serde_json::from_str(metadata)?;
        let build_dir = metadata["build_directory"]
            .as_str()
            .ok_or_else(|| DevtoolError::Usage("Cargo metadata has no build directory".into()))?
            .to_owned();
        Ok(Self {
            build_dir,
            architecture_selector,
            base: {
                let command = CommandSpec::new(
                    "git",
                    vec![
                        "rev-parse".into(),
                        "--verify".into(),
                        format!("{base}^{{commit}}"),
                    ],
                );
                let result = output(&command, Path::new("."))?;
                if !result.status.success() {
                    return Err(failed(&command, &result));
                }
                String::from_utf8_lossy(&result.stdout).trim().into()
            },
            root: PathBuf::from(String::from_utf8_lossy(&root.stdout).trim()),
            scratch,
            worktree: None,
        })
    }

    /// Runs a gate and requires positive evidence that each failure also occurs on base.
    pub(crate) fn verify(&mut self, mut command: CommandSpec) -> Result<(), DevtoolError> {
        if command.program == "cargo" {
            command
                .env
                .push(("CARGO_BUILD_BUILD_DIR".into(), self.build_dir.clone()));
        }
        let (head_command, report) = prepare(&command, &self.root, self.scratch.path(), "head")?;
        let head = output(&head_command, &self.root)?;
        print_output(&head);
        if head.status.success() {
            return Ok(());
        }
        if report.is_some() && head.status.code() != Some(100) {
            return Err(failed(&command, &head));
        }
        let Some(failures) = signatures(
            &command,
            &head,
            report.as_deref(),
            &self.root,
            &self.architecture_selector,
        )?
        else {
            return Err(failed(&command, &head));
        };
        if failures.is_empty() {
            return Err(failed(&command, &head));
        }
        let base_root = self.base_worktree()?;
        let mut base_command = command.clone();
        for argument in &mut base_command.args {
            *argument = remap_argument(argument, &self.root, &base_root)?;
        }

        if command.args.starts_with(&["nextest".into(), "run".into()]) {
            base_command
                .args
                .extend(["-E".into(), test_filter(&failures)]);
        } else if command.program == "cargo"
            && command.args.first().is_some_and(|arg| arg == "test")
        {
            for name in &failures {
                let mut single = crate::cargo_failures::rerun(name)?;
                single.env = base_command.env.clone();
                let (single, _) = prepare(&single, &base_root, self.scratch.path(), "base-test")?;
                let base = output(&single, &base_root)?;
                print_output(&base);
                if base.status.success()
                    || !signatures(
                        &single,
                        &base,
                        None,
                        &base_root,
                        &self.architecture_selector,
                    )?
                    .is_some_and(|existing| existing.contains(name))
                {
                    return Err(failed(&command, &head));
                }
                println!("already failing on base: {name}");
            }
            return Ok(());
        } else if command.program == "go" && command.args.iter().any(|arg| arg == "test") {
            for signature in &failures {
                let Some((package, test)) = signature.split_once('\t') else {
                    return Err(failed(&command, &head));
                };
                // Parent failures are summaries of failing subtests, not separate tests to rerun.
                if failures
                    .iter()
                    .any(|other| other.starts_with(&format!("{signature}/")))
                {
                    continue;
                }
                let pattern = test
                    .split('/')
                    .map(|part| format!("^{}$", regex::escape(part)))
                    .collect::<Vec<_>>()
                    .join("/");
                let mut single = base_command.clone();
                for argument in &mut single.args {
                    if argument == "./..." {
                        *argument = package.into();
                    }
                }
                single.args.extend(["-run".into(), pattern]);
                let (single, _) = prepare(&single, &base_root, self.scratch.path(), "base-go")?;
                let base = output(&single, &base_root)?;
                print_output(&base);
                if base.status.success()
                    || !signatures(
                        &single,
                        &base,
                        None,
                        &base_root,
                        &self.architecture_selector,
                    )?
                    .is_some_and(|existing| existing.contains(signature))
                {
                    return Err(failed(&command, &head));
                }
                println!("already failing on base: {signature}");
            }
            return Ok(());
        }
        let (base_command, report) =
            prepare(&base_command, &base_root, self.scratch.path(), "base")?;
        let base = output(&base_command, &base_root)?;
        print_output(&base);
        if report.is_some() && base.status.code() != Some(100) {
            return Err(failed(&command, &head));
        }
        let Some(existing) = signatures(
            &command,
            &base,
            report.as_deref(),
            &base_root,
            &self.architecture_selector,
        )?
        else {
            return Err(failed(&command, &head));
        };
        if base.status.success() || !failures.is_subset(&existing) {
            return Err(failed(&command, &head));
        }
        for failure in failures {
            println!("already failing on base: {failure}");
        }
        Ok(())
    }

    /// Checks out the exact base once, keeping intermediates in Cargo's configured shared cache.
    fn base_worktree(&mut self) -> Result<PathBuf, DevtoolError> {
        if let Some(path) = &self.worktree {
            return Ok(path.clone());
        }
        let path = self.scratch.path().join("base");
        let command = CommandSpec::new(
            "git",
            vec![
                "worktree".into(),
                "add".into(),
                "--detach".into(),
                path.display().to_string(),
                self.base.clone(),
            ],
        );
        let result = output(&command, &self.root)?;
        if !result.status.success() {
            return Err(failed(&command, &result));
        }
        self.worktree = Some(path.clone());
        Ok(path)
    }
}

impl Drop for Comparison {
    fn drop(&mut self) {
        if let Some(path) = &self.worktree {
            let _ = Command::new("git")
                .current_dir(&self.root)
                .args(["worktree", "remove", "--force"])
                .arg(path)
                .output();
        }
    }
}

/// Remaps Cargo's file-URL package selectors as well as ordinary paths, including encoded spaces.
fn remap_argument(argument: &str, head: &Path, base: &Path) -> Result<String, DevtoolError> {
    if argument.starts_with("path+file://") {
        let old = url_path(head);
        let new = url_path(base);
        let remapped = argument.replace(&old, &new);
        if remapped == argument {
            return Err(DevtoolError::Usage(
                "cannot remap workspace package identity to base".into(),
            ));
        }
        return Ok(remapped);
    }
    Ok(argument.replace(&*head.to_string_lossy(), &base.to_string_lossy()))
}

/// Encodes only file-path bytes, preserving URL separators on Unix and Windows.
fn url_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"/-_.~:".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// Enables structured results without changing repository configuration.
fn prepare(
    command: &CommandSpec,
    root: &Path,
    scratch: &Path,
    name: &str,
) -> Result<(CommandSpec, Option<PathBuf>), DevtoolError> {
    let mut command = command.clone();
    if command.program == "cargo" {
        command.env.push((
            "CARGO_TARGET_DIR".into(),
            root.join("target").display().to_string(),
        ));
        if command.args.first().is_some_and(|arg| arg == "clippy") {
            // Clippy replaces the workspace wrapper; expand this template in each checkout.
            let build_dir = command
                .env
                .iter()
                .rev()
                .find(|(key, _)| key == "CARGO_BUILD_BUILD_DIR")
                .map(|(_, value)| value.clone())
                .or_else(|| std::env::var("CARGO_BUILD_BUILD_DIR").ok())
                .unwrap_or_else(|| "{cargo-cache-home}/build".into());
            command.env.push((
                "CARGO_BUILD_BUILD_DIR".into(),
                Path::new(&build_dir)
                    .join("clippy/{workspace-path-hash}")
                    .display()
                    .to_string(),
            ));
        } else {
            isolate_workspace_artifacts(&mut command, root)?;
        }
    }
    if command.args.starts_with(&["nextest".into(), "run".into()]) {
        let report = scratch.join(format!("{name}.xml"));
        if report.exists() {
            fs::remove_file(&report).map_err(|source| DevtoolError::Spawn {
                command: "discard previous nextest report".into(),
                source,
            })?;
        }
        let config = scratch.join(format!("{name}.toml"));
        let mut text = fs::read_to_string(root.join(".config/nextest.toml")).unwrap_or_default();
        text.push_str(&format!(
            "\n[profile.devtool.junit]\npath = {}\n",
            serde_json::to_string(&report.display().to_string())?
        ));
        fs::write(&config, text).map_err(|source| DevtoolError::Spawn {
            command: "write nextest configuration".into(),
            source,
        })?;
        command.args.extend([
            "--config-file".into(),
            config.display().to_string(),
            "--profile".into(),
            "devtool".into(),
        ]);
        return Ok((command, Some(report)));
    }
    if command.program == "cargo"
        && command
            .args
            .first()
            .is_some_and(|arg| arg == "clippy" || arg == "test")
    {
        command.args.insert(1, "--message-format=json".into());
    }
    if command.program == "go" && command.args.iter().any(|arg| arg == "test") {
        command.args.push("-json".into());
    }
    Ok((command, None))
}

/// Gives head and base separate workspace hashes while retaining the shared dependency cache.
fn isolate_workspace_artifacts(command: &mut CommandSpec, root: &Path) -> Result<(), DevtoolError> {
    let directory = root.join("target/devtool");
    fs::create_dir_all(&directory).map_err(|source| DevtoolError::Spawn {
        command: "create comparison compiler directory".into(),
        source,
    })?;
    let wrapper = directory.join(if cfg!(windows) {
        "rustc.cmd"
    } else {
        "rustc.py"
    });
    let previous = command
        .env
        .iter()
        .rev()
        .find(|(key, _)| {
            key == "RUSTC_WORKSPACE_WRAPPER" || key == "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"
        })
        .map(|(_, value)| value.clone())
        .or_else(|| std::env::var("RUSTC_WORKSPACE_WRAPPER").ok())
        .or_else(|| std::env::var("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER").ok())
        .unwrap_or_default();
    let generator = Path::new(env!("CARGO_MANIFEST_DIR")).join("../compiler_workspace_wrapper.py");
    let result = Command::new(if cfg!(windows) { "python" } else { "python3" })
        .arg(generator)
        .arg(&wrapper)
        .arg(previous)
        .output()
        .map_err(|source| DevtoolError::Spawn {
            command: "create comparison compiler wrapper".into(),
            source,
        })?;
    if !result.status.success() {
        return Err(DevtoolError::Usage(
            String::from_utf8_lossy(&result.stderr).trim().into(),
        ));
    }
    let wrapper = String::from_utf8(result.stdout)
        .map_err(|_| DevtoolError::Usage("compiler wrapper path is not UTF-8".into()))?;
    command
        .env
        .push(("RUSTC_WORKSPACE_WRAPPER".into(), wrapper.trim().into()));
    Ok(())
}

/// Retains repeated diagnostics so an additional occurrence cannot match a single old error.
fn numbered_signatures(values: Vec<String>) -> BTreeSet<String> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    values
        .into_iter()
        .map(|value| {
            let count = counts.entry(value.clone()).or_default();
            *count += 1;
            format!("{value} (occurrence {count})")
        })
        .collect()
}

/// Returns the exact binary and test names of final JUnit failures, excluding flaky passes.
fn junit_failures(xml: &str) -> Result<BTreeSet<String>, DevtoolError> {
    let mut reader = Reader::from_str(xml);
    let mut current = None;
    let mut failures = BTreeSet::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.name().as_ref() == b"testcase" => {
                let mut name = String::new();
                let mut binary = String::new();
                for attribute in event.attributes().flatten() {
                    let value = attribute
                        .decode_and_unescape_value(reader.decoder())
                        .map_err(|error| DevtoolError::Usage(error.to_string()))?
                        .into_owned();
                    match attribute.key.as_ref() {
                        b"name" => name = value,
                        b"classname" => binary = value,
                        _ => {}
                    }
                }
                if name.is_empty() || binary.is_empty() {
                    return Err(DevtoolError::Usage(
                        "JUnit testcase has no complete test identity".into(),
                    ));
                }
                current = Some(format!("{binary}\t{name}"));
            }
            Ok(Event::Start(event) | Event::Empty(event))
                if matches!(event.name().as_ref(), b"failure" | b"error") =>
            {
                if let Some(name) = &current {
                    failures.insert(name.clone());
                } else {
                    return Err(DevtoolError::Usage(
                        "JUnit failure has no owning testcase".into(),
                    ));
                }
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"testcase" => current = None,
            Ok(Event::Eof) => break,
            Err(error) => return Err(DevtoolError::Usage(format!("invalid JUnit: {error}"))),
            _ => {}
        }
    }
    Ok(failures)
}

/// Builds exact filters so identically named tests in other binaries do not mask regressions.
fn test_filter(failures: &BTreeSet<String>) -> String {
    failures
        .iter()
        .filter_map(|name| name.split_once('\t'))
        .map(|(binary, name)| {
            format!(
                "(binary_id(={}) & test(={}))",
                nextest_matcher(binary),
                nextest_matcher(name)
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Escapes matcher values with nextest's grammar, which has no embedded quote syntax.
fn nextest_matcher(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || "_:-.".contains(character) {
                character.to_string()
            } else {
                format!("\\u{{{:x}}}", character as u32)
            }
        })
        .collect()
}

/// Executes a check in its own worktree with explicit command environment overrides.
fn output(command: &CommandSpec, root: &Path) -> Result<Output, DevtoolError> {
    if command.program == "cargo" && command.args.first().is_some_and(|arg| arg == "test") {
        use std::io::{Read, Seek};
        let mut file = tempfile::tempfile().map_err(|source| DevtoolError::Spawn {
            command: "capture test output".into(),
            source,
        })?;
        let stdout = file.try_clone().map_err(|source| DevtoolError::Spawn {
            command: "capture test output".into(),
            source,
        })?;
        let stderr = file.try_clone().map_err(|source| DevtoolError::Spawn {
            command: "capture test output".into(),
            source,
        })?;
        let status = Command::new(&command.program)
            .args(&command.args)
            .current_dir(root)
            .env("CARGO_TERM_COLOR", "never")
            .envs(command.env.iter().map(|(key, value)| (key, value)))
            .stdout(stdout)
            .stderr(stderr)
            .status()
            .map_err(|source| DevtoolError::Spawn {
                command: command.to_string(),
                source,
            })?;
        file.rewind().map_err(|source| DevtoolError::Spawn {
            command: "read test output".into(),
            source,
        })?;
        let mut stdout = Vec::new();
        file.read_to_end(&mut stdout)
            .map_err(|source| DevtoolError::Spawn {
                command: "read test output".into(),
                source,
            })?;
        return Ok(Output {
            status,
            stdout,
            stderr: Vec::new(),
        });
    }
    Command::new(&command.program)
        .args(&command.args)
        .current_dir(root)
        .envs(command.env.iter().map(|(key, value)| (key, value)))
        .output()
        .map_err(|source| DevtoolError::Spawn {
            command: command.to_string(),
            source,
        })
}

/// Keeps the original output visible when reporting a comparison result.
fn print_output(output: &Output) {
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
}

/// Describes the failing gate without treating a rerun's exit code as proof of equivalence.
fn failed(command: &CommandSpec, output: &Output) -> DevtoolError {
    DevtoolError::Command {
        command: command.to_string(),
        status: output.status.to_string(),
        stderr: "new failure or base comparison unavailable".into(),
    }
}

#[cfg(test)]
#[path = "baseline/workspace_tests.rs"]
mod workspace_tests;

#[cfg(test)]
mod classification_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_failures_without_a_test_cannot_match_an_existing_go_failure() {
        let text = r#"{"Action":"fail","Package":"old","Test":"TestExisting"}
{"Action":"fail","Package":"old"}
{"Action":"fail","Package":"new"}
"#;
        let command = CommandSpec::new("go", vec!["test".into(), "./...".into()]);
        let mut output = Command::new("git").arg("--version").output().unwrap();
        output.stdout = text.lines().next().unwrap().as_bytes().to_vec();
        assert!(
            signatures(&command, &output, None, Path::new("/repo"), "architecture")
                .unwrap()
                .is_none()
        );
        output.stdout = text.as_bytes().to_vec();
        output.stderr.clear();
        assert!(
            signatures(&command, &output, None, Path::new("/repo"), "architecture")
                .unwrap()
                .is_none()
        );
        output.stdout = text
            .lines()
            .take(2)
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        assert_eq!(
            signatures(&command, &output, None, Path::new("/repo"), "architecture").unwrap(),
            Some(BTreeSet::from(["old\tTestExisting".into()]))
        );
    }

    #[test]
    fn final_failures_keep_binary_identity_and_ignore_flaky_passes() {
        let xml = r#"<testsuites><testsuite><testcase classname="crate::one" name="same"><failure/></testcase><testcase classname="crate::two" name="same"><flakyFailure/></testcase><testcase classname="crate::one" name="hang"><error/></testcase></testsuite></testsuites>"#;
        let failures = junit_failures(xml).unwrap();
        assert_eq!(
            failures,
            BTreeSet::from(["crate::one\thang".into(), "crate::one\tsame".into()])
        );
    }
    #[test]
    fn malformed_report_is_not_evidence_of_existing_failure() {
        assert!(junit_failures("<testcase><failure></testcase>").is_err());
    }
    #[test]
    fn package_ids_remap_encoded_paths_and_reject_unmapped_head_selectors() {
        assert_eq!(
            remap_argument(
                "path+file:///head%20space/crate#1",
                Path::new("/head space"),
                Path::new("/base")
            )
            .unwrap(),
            "path+file:///base/crate#1"
        );
        assert_eq!(
            remap_argument(
                "path+file:///C:/head%20space/crate#1",
                Path::new(r"C:\head space"),
                Path::new(r"D:\base")
            )
            .unwrap(),
            "path+file:///D:/base/crate#1"
        );
        assert!(
            remap_argument(
                "path+file:///other/crate#1",
                Path::new("/head"),
                Path::new("/base")
            )
            .is_err()
        );
    }

    #[test]
    fn repeated_diagnostics_do_not_hide_a_new_occurrence() {
        let base = numbered_signatures(vec!["same warning".into()]);
        let head = numbered_signatures(vec!["same warning".into(), "same warning".into()]);
        assert!(!head.is_subset(&base));
    }

    #[test]
    fn actual_nextest_reruns_only_matching_base_failures() {
        if !Command::new("cargo")
            .args(["nextest", "--version"])
            .output()
            .is_ok_and(|output| output.status.success())
        {
            eprintln!("missing fixture: cargo-nextest; skipping command compatibility test");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap().join("repo");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname='base-compare-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n").unwrap();
        fs::write(
            root.join("Cargo.lock"),
            "version = 4\n[[package]]\nname = \"base-compare-fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let existing = "#[test] fn existing_é() { panic!(\"existing failure\"); }\n";
        fs::write(root.join("src/lib.rs"), existing).unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .current_dir(&root)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        git(&["init", "--quiet"]);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ]);
        let scratch = tempfile::tempdir().unwrap();
        let mut comparison = Comparison {
            architecture_selector: "architecture".into(),
            base: "HEAD".into(),
            build_dir: temp.path().join("build").display().to_string(),
            root: root.clone(),
            scratch,
            worktree: None,
        };
        let command =
            CommandSpec::cargo(&["nextest", "run", "--locked", "--offline", "--no-fail-fast"]);
        assert!(comparison.verify(command.clone()).is_ok());
        fs::write(
            root.join("src/lib.rs"),
            format!("{existing}#[test] fn new_failure() {{ panic!(\"new failure\"); }}\n"),
        )
        .unwrap();
        assert!(comparison.verify(command).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn base_reruns_only_failure_filters_and_does_not_hide_new_failures() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir(&root).unwrap();
        let run = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .current_dir(&root)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        run(&["init", "--quiet"]);
        fs::write(root.join("base-failures"), "existing").unwrap();
        run(&["add", "base-failures"]);
        run(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ]);
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let cargo = bin.join("cargo");
        fs::write(&cargo, r#"#!/usr/bin/env python3
import os, pathlib, sys, tomllib
args = sys.argv[1:]
config = tomllib.loads(pathlib.Path(args[args.index('--config-file')+1]).read_text())
path = pathlib.Path(config['profile']['devtool']['junit']['path'])
is_base = pathlib.Path.cwd().name == 'base'
if is_base:
    if 'FIXTURE_BASE_EXIT' in os.environ:
        sys.exit(int(os.environ['FIXTURE_BASE_EXIT']))
    assert '-E' in args
    assert 'test(=existing)' in args[args.index('-E')+1]
    failures = pathlib.Path('base-failures').read_text().split()
else:
    failures = pathlib.Path('head-failures').read_text().split()
path.write_text('<testsuites><testsuite>' + ''.join('<testcase classname="fixture::lib" name="'+name+'"><failure/></testcase>' for name in failures) + '</testsuite></testsuites>')
sys.exit(int(os.environ.get("FIXTURE_EXIT", "100")) if failures else 0)
"#).unwrap();
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = CommandSpec::cargo(&["nextest", "run", "--no-fail-fast"]);
        command.env.push((
            "PATH".into(),
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        ));
        let scratch = tempfile::tempdir().unwrap();
        let mut comparison = Comparison {
            architecture_selector: "architecture".into(),
            base: "HEAD".into(),
            build_dir: temp.path().join("build").display().to_string(),
            root: root.clone(),
            scratch,
            worktree: None,
        };
        fs::write(root.join("head-failures"), "existing").unwrap();
        assert!(comparison.verify(command.clone()).is_ok());
        for code in ["101", "100"] {
            let mut stale_base = command.clone();
            stale_base
                .env
                .push(("FIXTURE_BASE_EXIT".into(), code.into()));
            assert!(comparison.verify(stale_base).is_err());
        }
        let mut infrastructure = command.clone();
        infrastructure
            .env
            .push(("FIXTURE_EXIT".into(), "101".into()));
        assert!(comparison.verify(infrastructure).is_err());
        fs::write(root.join("head-failures"), "existing new").unwrap();
        assert!(comparison.verify(command).is_err());
        let base_path = comparison.worktree.clone().unwrap();
        drop(comparison);
        assert!(!base_path.exists());
        assert_eq!(
            Command::new("git")
                .current_dir(root)
                .args(["worktree", "list", "--porcelain"])
                .output()
                .unwrap()
                .stdout
                .split(|byte| *byte == b'\n')
                .filter(|line| line.starts_with(b"worktree "))
                .count(),
            1
        );
    }
}
