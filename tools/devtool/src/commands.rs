use std::fmt;

use crate::{ExtraChecks, Package, Selection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestRunner {
    Cargo,
    Nextest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl CommandSpec {
    pub(crate) fn new(program: &str, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            env: Vec::new(),
        }
    }

    pub(crate) fn cargo(args: &[&str]) -> Self {
        Self::new(
            "cargo",
            args.iter().map(|argument| (*argument).into()).collect(),
        )
    }
}

impl fmt::Display for CommandSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (key, value) in &self.env {
            write!(formatter, "{key}={value} ")?;
        }
        write!(formatter, "{}", self.program)?;
        for argument in &self.args {
            write!(formatter, " {argument}")?;
        }
        Ok(())
    }
}

/// Builds verification commands while respecting each package's Cargo doctest setting.
/// Clippy type-checks every target, so there is no separate `cargo check`.
#[must_use]
pub fn verification_commands(
    selection: &Selection,
    runner: TestRunner,
    packages: &[Package],
) -> Vec<CommandSpec> {
    let doctest_filters: Vec<String> = packages
        .iter()
        .filter(|package| {
            package.doctest
                && match selection {
                    Selection::Workspace => true,
                    Selection::Packages(names) => names.contains(&package.name),
                    Selection::NoPackages => false,
                }
        })
        .flat_map(|package| ["-p".into(), package.cargo_id.clone()])
        .collect();
    let mut commands = vec![
        CommandSpec::cargo(&["fmt", "--all", "--", "--check"]),
        CommandSpec::cargo(&[
            "run",
            "-p",
            package_spec("architecture", packages),
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
            append_tests(&mut commands, runner, &[], true, &doctest_filters);
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
        Selection::Packages(names) => {
            let mut filters = Vec::with_capacity(names.len() * 2);
            for name in names {
                filters.extend(["-p".into(), package_spec(name, packages).to_owned()]);
            }
            let mut clippy = vec!["clippy".into(), "--locked".into()];
            clippy.extend(filters.clone());
            clippy.extend([
                "--all-targets".into(),
                "--".into(),
                "-D".into(),
                "warnings".into(),
            ]);
            append_tests(&mut commands, runner, &filters, false, &doctest_filters);
            commands.push(CommandSpec::new("cargo", clippy));
        }
    }
    commands
}

/// Runs `go test` and `go vet` per selected module, then the packaging tests.
#[must_use]
pub fn extra_commands(checks: &ExtraChecks) -> Vec<CommandSpec> {
    let mut commands = Vec::new();
    for module in &checks.go_modules {
        let dir = if module.dir.is_empty() {
            "."
        } else {
            &module.dir
        };
        for action in ["test", "vet"] {
            let mut command = CommandSpec::new(
                "go",
                vec!["-C".into(), dir.into(), action.into(), "./...".into()],
            );
            if !module.in_workspace {
                command.env.push(("GOWORK".into(), "off".into()));
            }
            commands.push(command);
        }
    }
    if checks.packaging {
        commands.push(CommandSpec::new(
            "python3",
            [
                "-m",
                "unittest",
                "discover",
                "-s",
                "packaging/tests",
                "-p",
                "test_*.py",
            ]
            .map(String::from)
            .to_vec(),
        ));
    }
    commands
}

/// Uses Cargo's exact workspace identity so a registry package with the same name is unambiguous.
pub(crate) fn package_spec<'a>(name: &'a str, packages: &'a [Package]) -> &'a str {
    packages
        .iter()
        .find(|package| package.name == name)
        .map_or(name, |package| package.cargo_id.as_str())
}

/// Adds the selected test runner and only the doctests enabled by package metadata.
fn append_tests(
    commands: &mut Vec<CommandSpec>,
    runner: TestRunner,
    filters: &[String],
    workspace: bool,
    doctest_filters: &[String],
) {
    match runner {
        TestRunner::Cargo => {
            let mut args = vec!["test".into(), "--no-fail-fast".into()];
            if workspace {
                args.push("--workspace".into());
            }
            args.push("--locked".into());
            args.extend_from_slice(filters);
            commands.push(CommandSpec::new("cargo", args));
        }
        TestRunner::Nextest => {
            let mut args = vec!["nextest".into(), "run".into(), "--no-fail-fast".into()];
            if workspace {
                args.push("--workspace".into());
            }
            args.push("--locked".into());
            args.extend_from_slice(filters);
            commands.push(CommandSpec::new("cargo", args));
            if doctest_filters.is_empty() {
                return;
            }
            let mut args = vec!["test".into(), "--doc".into(), "--locked".into()];
            args.extend_from_slice(doctest_filters);
            commands.push(CommandSpec::new("cargo", args));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TestRunner, extra_commands, verification_commands};
    use crate::{ExtraChecks, GoModule, Selection};

    #[test]
    fn package_commands_are_batched_and_strict() {
        let commands = verification_commands(
            &Selection::Packages(vec!["assets".into(), "render".into()]),
            TestRunner::Cargo,
            &[],
        );
        assert_eq!(commands[0].to_string(), "cargo fmt --all -- --check");
        assert_eq!(
            commands[1].to_string(),
            "cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml"
        );
        assert_eq!(
            commands[2].to_string(),
            "cargo test --no-fail-fast --locked -p assets -p render"
        );
        assert_eq!(
            commands[3].to_string(),
            "cargo clippy --locked -p assets -p render --all-targets -- -D warnings"
        );
        assert_eq!(commands.len(), 4);
    }

    #[test]
    fn workspace_selection_uses_full_workspace_commands() {
        let commands = verification_commands(&Selection::Workspace, TestRunner::Cargo, &[]);
        assert_eq!(
            commands[2].to_string(),
            "cargo test --no-fail-fast --workspace --locked"
        );
        assert_eq!(
            commands[3].to_string(),
            "cargo clippy --workspace --all-targets --locked -- -D warnings"
        );
        assert!(commands.iter().all(|command| command.args[0] != "check"));
    }

    #[test]
    fn documentation_selection_runs_only_repository_checks() {
        let commands = verification_commands(&Selection::NoPackages, TestRunner::Cargo, &[]);
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn nextest_runner_keeps_doctests_in_the_fast_gate() {
        let commands = verification_commands(
            &Selection::Packages(vec!["world".into()]),
            TestRunner::Nextest,
            &[crate::Package::from_owned(
                "world".into(),
                "world".into(),
                "crates/world".into(),
                vec![],
                true,
            )],
        );
        assert_eq!(
            commands[2].to_string(),
            "cargo nextest run --no-fail-fast --locked -p world"
        );
        assert_eq!(
            commands[3].to_string(),
            "cargo test --doc --locked -p world"
        );
    }

    #[test]
    fn go_modules_outside_the_workspace_disable_go_work() {
        let commands = extra_commands(&ExtraChecks {
            go_modules: vec![
                GoModule::new("core", true),
                GoModule::new("tools/localserver", false),
            ],
            packaging: true,
        });
        let commands: Vec<_> = commands.iter().map(ToString::to_string).collect();
        assert_eq!(
            commands,
            [
                "go -C core test ./...",
                "go -C core vet ./...",
                "GOWORK=off go -C tools/localserver test ./...",
                "GOWORK=off go -C tools/localserver vet ./...",
                "python3 -m unittest discover -s packaging/tests -p test_*.py",
            ]
        );
        assert!(extra_commands(&ExtraChecks::default()).is_empty());
    }
}
