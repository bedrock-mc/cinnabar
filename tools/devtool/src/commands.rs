use std::fmt;

use crate::Selection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestRunner {
    Cargo,
    Nextest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

impl CommandSpec {
    pub(crate) fn cargo(args: &[&str]) -> Self {
        Self {
            program: "cargo".into(),
            args: args.iter().map(|argument| (*argument).into()).collect(),
        }
    }
}

impl fmt::Display for CommandSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.program)?;
        for argument in &self.args {
            write!(formatter, " {argument}")?;
        }
        Ok(())
    }
}

#[must_use]
pub fn verification_commands(selection: &Selection, runner: TestRunner) -> Vec<CommandSpec> {
    let mut commands = vec![
        CommandSpec::cargo(&["fmt", "--all", "--", "--check"]),
        CommandSpec::cargo(&[
            "run",
            "-p",
            "architecture",
            "--locked",
            "--",
            "check",
            "--root",
            ".",
            "--policy",
            "tools/architecture/policy.toml",
        ]),
    ];
    match selection {
        Selection::NoPackages => {}
        Selection::Workspace => {
            commands.push(CommandSpec::cargo(&["check", "--workspace", "--locked"]));
            append_tests(&mut commands, runner, &[], true);
            commands.push(CommandSpec::cargo(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--locked",
                "--",
                "-D",
                "warnings",
            ]));
        }
        Selection::Packages(packages) => {
            let mut filters = Vec::with_capacity(packages.len() * 2);
            for package in packages {
                filters.extend(["-p".into(), package.clone()]);
            }
            let mut check = vec!["check".into(), "--locked".into()];
            check.extend(filters.clone());
            let mut clippy = vec!["clippy".into(), "--locked".into()];
            clippy.extend(filters.clone());
            clippy.extend([
                "--all-targets".into(),
                "--".into(),
                "-D".into(),
                "warnings".into(),
            ]);
            commands.push(CommandSpec {
                program: "cargo".into(),
                args: check,
            });
            append_tests(&mut commands, runner, &filters, false);
            commands.push(CommandSpec {
                program: "cargo".into(),
                args: clippy,
            });
        }
    }
    commands
}

fn append_tests(
    commands: &mut Vec<CommandSpec>,
    runner: TestRunner,
    filters: &[String],
    workspace: bool,
) {
    match runner {
        TestRunner::Cargo => {
            let mut args = vec!["test".into()];
            if workspace {
                args.push("--workspace".into());
            }
            args.push("--locked".into());
            args.extend_from_slice(filters);
            commands.push(CommandSpec {
                program: "cargo".into(),
                args,
            });
        }
        TestRunner::Nextest => {
            let mut args = vec!["nextest".into(), "run".into()];
            if workspace {
                args.push("--workspace".into());
            }
            args.push("--locked".into());
            args.extend_from_slice(filters);
            commands.push(CommandSpec {
                program: "cargo".into(),
                args,
            });
            let mut args = vec!["test".into(), "--doc".into()];
            if workspace {
                args.push("--workspace".into());
            }
            args.push("--locked".into());
            args.extend_from_slice(filters);
            commands.push(CommandSpec {
                program: "cargo".into(),
                args,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TestRunner, verification_commands};
    use crate::Selection;

    #[test]
    fn package_commands_are_batched_and_strict() {
        let commands = verification_commands(
            &Selection::Packages(vec!["assets".into(), "render".into()]),
            TestRunner::Cargo,
        );
        assert_eq!(commands[0].to_string(), "cargo fmt --all -- --check");
        assert_eq!(
            commands[1].to_string(),
            "cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml"
        );
        assert_eq!(
            commands[2].to_string(),
            "cargo check --locked -p assets -p render"
        );
        assert_eq!(
            commands[3].to_string(),
            "cargo test --locked -p assets -p render"
        );
        assert_eq!(
            commands[4].to_string(),
            "cargo clippy --locked -p assets -p render --all-targets -- -D warnings"
        );
    }

    #[test]
    fn workspace_selection_uses_full_workspace_commands() {
        let commands = verification_commands(&Selection::Workspace, TestRunner::Cargo);
        assert_eq!(commands[2].to_string(), "cargo check --workspace --locked");
        assert_eq!(commands[3].to_string(), "cargo test --workspace --locked");
        assert_eq!(
            commands[4].to_string(),
            "cargo clippy --workspace --all-targets --locked -- -D warnings"
        );
    }

    #[test]
    fn documentation_selection_runs_only_repository_checks() {
        let commands = verification_commands(&Selection::NoPackages, TestRunner::Cargo);
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn nextest_runner_keeps_doctests_in_the_fast_gate() {
        let commands = verification_commands(
            &Selection::Packages(vec!["world".into()]),
            TestRunner::Nextest,
        );
        assert_eq!(
            commands[3].to_string(),
            "cargo nextest run --locked -p world"
        );
        assert_eq!(
            commands[4].to_string(),
            "cargo test --doc --locked -p world"
        );
    }
}
