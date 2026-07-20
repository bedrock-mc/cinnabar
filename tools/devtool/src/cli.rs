use std::process::{Command, Stdio};

use crate::{
    CommandSpec, DevtoolError, Selection, TestRunner, packages_from_metadata, select_packages,
    selection::normalize, verification_commands,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub base: String,
    pub dry_run: bool,
}

pub fn parse_args<I, S>(arguments: I) -> Result<Options, DevtoolError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut arguments = arguments.into_iter().map(Into::into);
    if arguments.next().as_deref() != Some("verify-affected") {
        return Err(DevtoolError::Usage(
            "expected `verify-affected` subcommand".into(),
        ));
    }
    let mut base = None;
    let mut dry_run = false;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--base" => {
                let value = arguments.next().ok_or_else(|| {
                    DevtoolError::Usage("`--base` requires a Git reference".into())
                })?;
                if value.is_empty() {
                    return Err(DevtoolError::Usage(
                        "`--base` requires a nonempty Git reference".into(),
                    ));
                }
                base = Some(value);
            }
            "--dry-run" => dry_run = true,
            unknown => {
                return Err(DevtoolError::Usage(format!("unknown argument `{unknown}`")));
            }
        }
    }
    Ok(Options {
        base: base.ok_or_else(|| DevtoolError::Usage("missing required `--base`".into()))?,
        dry_run,
    })
}

pub fn run(options: &Options) -> Result<(), DevtoolError> {
    let metadata = capture(CommandSpec::cargo(&[
        "metadata",
        "--format-version",
        "1",
        "--no-deps",
        "--locked",
    ]))?;
    let packages = packages_from_metadata(&metadata)?;
    let mut changed = nul_paths(&capture(CommandSpec {
        program: "git".into(),
        args: vec![
            "diff".into(),
            "--name-only".into(),
            "-z".into(),
            options.base.clone(),
            "--".into(),
        ],
    })?);
    changed.extend(nul_paths(&capture(CommandSpec {
        program: "git".into(),
        args: vec![
            "ls-files".into(),
            "--others".into(),
            "--exclude-standard".into(),
            "-z".into(),
        ],
    })?));
    changed.sort();
    changed.dedup();
    let changed_refs = changed.iter().map(String::as_str).collect::<Vec<_>>();
    let selection = select_packages(&changed_refs, &packages);
    match &selection {
        Selection::Workspace => println!("affected: workspace"),
        Selection::Packages(packages) => println!("affected: {}", packages.join(", ")),
        Selection::NoPackages => println!("affected: no Rust packages"),
    }
    let runner = detect_test_runner();
    match runner {
        TestRunner::Nextest => println!("test runner: cargo-nextest"),
        TestRunner::Cargo => {
            println!("test runner: cargo test (install cargo-nextest for faster local tests)");
        }
    }
    for command in verification_commands(&selection, runner) {
        println!("$ {command}");
        if !options.dry_run {
            execute(command)?;
        }
    }
    Ok(())
}

fn detect_test_runner() -> TestRunner {
    if Command::new("cargo")
        .args(["nextest", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        TestRunner::Nextest
    } else {
        TestRunner::Cargo
    }
}

fn nul_paths(output: &str) -> Vec<String> {
    output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(normalize)
        .collect()
}

fn capture(command: CommandSpec) -> Result<String, DevtoolError> {
    let display = command.to_string();
    let output = Command::new(&command.program)
        .args(&command.args)
        .output()
        .map_err(|source| DevtoolError::Spawn {
            command: display.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(DevtoolError::Command {
            command: display,
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
        });
    }
    String::from_utf8(output.stdout).map_err(|_| DevtoolError::NonUtf8 { command: display })
}

fn execute(command: CommandSpec) -> Result<(), DevtoolError> {
    let display = command.to_string();
    let status = Command::new(&command.program)
        .args(&command.args)
        .status()
        .map_err(|source| DevtoolError::Spawn {
            command: display.clone(),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(DevtoolError::Command {
            command: display,
            status: status.to_string(),
            stderr: "see command output above".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    #[test]
    fn command_line_requires_an_explicit_base_and_supports_dry_run() {
        let options = parse_args(["verify-affected", "--base", "origin/main", "--dry-run"])
            .expect("parse options");
        assert_eq!(options.base, "origin/main");
        assert!(options.dry_run);

        assert!(parse_args(["verify-affected", "--dry-run"]).is_err());
        assert!(parse_args(["unknown", "--base", "HEAD"]).is_err());
    }
}
