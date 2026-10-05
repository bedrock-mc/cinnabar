mod cli;
mod commands;
mod metadata;
mod selection;

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
    #[error("{0}\nusage: devtool verify-affected --base <git-ref> [--dry-run]")]
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
