//! Checks external executables and unusable inherited wrapper records.
#[cfg(unix)]
mod support;

#[cfg(unix)]
#[test]
fn external_binary_target_survives_nested_wrappers() {
    use std::process::Command;
    let temp = tempfile::tempdir().unwrap();
    let tools = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let result = support::bounded_output(Command::new("python3").args(["-c", r#"
import pathlib, subprocess, sys
sys.path.insert(0, sys.argv[1])
from compiler_workspace_wrapper import workspace_wrapper
root = pathlib.Path(sys.argv[2])
external = root / 'target/devtool/rustc-external.py'
external.parent.mkdir(parents=True)
external.write_text('#!/usr/bin/env python3\nimport os, sys\nos.execv(sys.executable, [sys.executable, *sys.argv[1:]])\n')
external.chmod(0o700)
target = workspace_wrapper(root / 'output.py', str(external))
subprocess.run([str(target), '-c', "import os; assert os.environ['CARGO_BIN_EXE_test-client'] == 'kept'"], check=True, timeout=3)
for levels in (2, 3):
    target = sys.executable
    for level in range(levels):
        target = str(workspace_wrapper(pathlib.Path(sys.argv[2]) / str(level) / 'rustc.py', target))
    subprocess.run([target, '-c', "import os; assert os.environ['CARGO_BIN_EXE_test-client'] == 'kept'"], check=True, timeout=3)
"#]).arg(tools).arg(temp.path()).env("CARGO_BIN_EXE_test-client", "kept"));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[cfg(unix)]
#[test]
fn cycles_and_missing_owned_records_fail_without_forwarding() {
    use std::process::Command;
    let temp = tempfile::tempdir().unwrap();
    let tools = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let result = support::bounded_output(
        Command::new("python3")
            .args([
                "-c",
                r#"
import json, pathlib, sys
sys.path.insert(0, sys.argv[1])
from compiler_workspace_wrapper import MARKER, workspace_wrapper
root = pathlib.Path(sys.argv[2])
first, second = root / 'first', root / 'second'
first.write_text('# ' + MARKER + json.dumps(str(second)) + '\n')
second.write_text('# ' + MARKER + json.dumps(str(first)) + '\n')
for inner in (first, root / 'target/devtool/rustc.py'):
    try:
        workspace_wrapper(root / 'output', str(inner))
    except ValueError:
        pass
    else:
        raise AssertionError('forwarded an unusable wrapper')
"#,
            ])
            .arg(tools)
            .arg(temp.path()),
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
