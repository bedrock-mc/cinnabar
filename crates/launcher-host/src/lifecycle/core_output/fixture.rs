//! `crashcore`, the Go stand-in for bedrock-core in `core/internal/corelog/testdata`, built once
//! per test process with the workspace's Go toolchain.

use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
};

static CRASH_CORE: LazyLock<Option<PathBuf>> = LazyLock::new(build);

/// The built fixture; `None`, after printing the missing-fixture message, without `go`.
pub(crate) fn crash_core() -> Option<&'static Path> {
    CRASH_CORE.as_deref()
}

fn build() -> Option<PathBuf> {
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core");
    let dir = std::env::temp_dir().join(format!("cinnabar-crashcore-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let executable = dir.join(format!("crashcore{}", std::env::consts::EXE_SUFFIX));
    let output = Command::new("go")
        .current_dir(&core)
        .args(["build", "-trimpath", "-o"])
        .arg(&executable)
        .arg("./internal/corelog/testdata/crashcore")
        .output();
    match output {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            println!("skipping: missing fixture `go` toolchain to build crashcore ({error})");
            None
        }
        Err(error) => panic!("run go build for crashcore: {error}"),
        Ok(output) => {
            assert!(
                output.status.success(),
                "go build crashcore failed with {}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
            Some(executable)
        }
    }
}

/// A command running the fixture in `mode` (`clean`, `fail` or `panic`).
pub(crate) fn command(executable: &Path, mode: &str) -> Command {
    let mut command = Command::new(executable);
    command
        .args(["-mode", mode])
        .stdin(std::process::Stdio::null());
    command
}
