mod capture;
mod cli;
mod commands;
mod diagnostic_bundle;
mod metadata;
mod selection;
mod worktrees;

use std::{io, path::PathBuf};

use thiserror::Error;

pub use cli::{Options, parse_args, run};
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
        "{0}\nusage: devtool verify-affected --base <git-ref> [--dry-run] | wt-gc [--apply] [--force <path>] | diag [--data-root <path>] | capture | trace-summary --trace <file>"
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
}

/// Dispatches repository commands without requiring a Git base for utility commands.
pub fn dispatch(args: Vec<String>) -> Result<(), DevtoolError> {
    match args.first().map(String::as_str) {
        Some("capture") => capture::run(&args[1..], false),
        Some("trace-summary") => capture::run(&args[1..], true),
        Some("diag") => diagnostic_bundle::run(&args[1..]),
        Some("wt-gc") => worktrees::run(&worktrees::parse(&args[1..])?),
        _ => parse_args(args).and_then(|options| run(&options)),
    }
}
