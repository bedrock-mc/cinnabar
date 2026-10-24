#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "devtool commands report selections, summaries and replayed command output on the terminal"
)]

mod baseline;
mod capture;
mod cargo_failures;
mod ci;
mod cli;
mod commands;
mod diagnostic_bundle;
mod metadata;
mod selection;
mod startup;
mod symbolicate;
mod web;
mod worktrees;

use std::{io, path::PathBuf};

use thiserror::Error;

pub use ci::{CiPlan, plan_ci};
pub use cli::{Options, Subcommand, parse_args, run};
pub use commands::{CommandSpec, TestRunner, extra_commands, verification_commands};
pub use metadata::{go_modules, packages_from_metadata};
pub use selection::{
    ExtraChecks, GoModule, Package, Selection, select_extra_checks, select_packages,
};

#[derive(Debug, Error)]
pub enum DevtoolError {
    #[error("failed to parse Cargo metadata: {0}")]
    Metadata(#[from] serde_json::Error),
    #[error("workspace package manifest `{manifest}` is outside workspace root `{root}`")]
    ManifestOutsideWorkspace { manifest: PathBuf, root: PathBuf },
    #[error("workspace package manifest `{0}` has no parent directory")]
    ManifestWithoutParent(PathBuf),
    #[error(
        "{0}\nusage: devtool verify-affected --base <git-ref> [--dry-run] | ci-plan --base <git-ref> | startup [--client <release-client>] [--runs <N>] | capture | trace-summary --trace <file> | wt-gc | diag [--data-root <path>] | symbolicate <crash folder|minidump> [--symbols <dir|zip>] | check-web | build-web --assets <dir> --out <new-dir> [--release]"
    )]
    Usage(String),
    #[error("failed to run `{command}`: {source}")]
    Spawn { command: String, source: io::Error },
    #[error("`{command}` failed with status {status}: {stderr}")]
    Command {
        command: String,
        status: String,
        stderr: String,
    },
    #[error("`{command}` produced non-UTF-8 output")]
    NonUtf8 { command: String },
    /// A crash input or symbol source that `symbolicate` cannot read.
    #[error("{0}")]
    Symbolicate(String),
    #[error("web bundle: {0}")]
    Web(String),
}

/// Dispatches repository commands without requiring a Git base for utility commands.
pub fn dispatch(args: Vec<String>) -> Result<(), DevtoolError> {
    match args.first().map(String::as_str) {
        Some("diag") => diagnostic_bundle::run(&args[1..]),
        Some("startup") => startup::run(&args[1..]),
        Some("capture") => capture::run(&args[1..], false),
        Some("trace-summary") => capture::run(&args[1..], true),
        Some("wt-gc") => worktrees::run(&args[1..]),
        Some("symbolicate") => symbolicate::run(&args[1..]),
        Some("build-web") => web::run(&args[1..]),
        Some("check-web") => web::check(&args[1..]),
        _ => parse_args(args).and_then(|options| run(&options)),
    }
}
