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

#[cfg(unix)]
#[test]
fn cargo_config_wrapper_precedence_preserves_relative_paths() {
    use std::process::Command;
    let temp = tempfile::tempdir().unwrap();
    let tools = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let result = support::bounded_output(Command::new("python3")
        .args(["-c", r#"
import os, pathlib, sys
sys.path.insert(0, sys.argv[1])
from compiler_workspace_wrapper import effective_workspace_wrapper
root = pathlib.Path(sys.argv[2]).resolve()
home = root / 'cargo-home'
home.mkdir()
(home / 'config.toml').write_text("[build]\nrustc-workspace-wrapper='global-wrapper'\n")
os.environ['CARGO_HOME'] = str(home)
os.chdir(root)
assert effective_workspace_wrapper([]) == 'global-wrapper'
config = root / '.cargo'
config.mkdir()
(config / 'config.toml').write_text("[build]\nrustc-workspace-wrapper='./local-wrapper'\n")
assert effective_workspace_wrapper([]) == str(root / 'local-wrapper')
(config / 'config').write_text("[build]\nrustc-workspace-wrapper='./legacy-wrapper'\n")
assert effective_workspace_wrapper([]) == str(root / 'legacy-wrapper')
shared = root / 'shared/settings'
shared.mkdir(parents=True)
shared_config = shared / 'config.toml'
shared_config.write_text("[build]\nrustc-workspace-wrapper='./symlink-wrapper'\n")
(config / 'config').unlink()
(config / 'config').symlink_to(shared_config)
assert effective_workspace_wrapper([]) == str(root / 'symlink-wrapper')
os.environ['CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER'] = './environment-wrapper'
assert effective_workspace_wrapper([]) == str(root / 'environment-wrapper')
arguments = ['check', '--config', "build.rustc-workspace-wrapper='./argument-wrapper'"]
assert effective_workspace_wrapper(arguments) == str(root / 'argument-wrapper')
extra = config / 'extra.toml'
extra.write_text("[build]\nrustc-workspace-wrapper='./file-wrapper'\n")
assert effective_workspace_wrapper(['check', '--config=' + str(extra)]) == str(root / 'file-wrapper')
os.environ['RUSTC_WORKSPACE_WRAPPER'] = './dedicated-wrapper'
assert effective_workspace_wrapper(arguments) == str(root / 'dedicated-wrapper')
os.environ['RUSTC_WORKSPACE_WRAPPER'] = ''
assert effective_workspace_wrapper(arguments) == ''
"#])
        .arg(tools).arg(temp.path())
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[cfg(unix)]
#[test]
fn symlinked_external_wrappers_run_through_their_invoked_path() {
    use std::process::Command;
    let temp = tempfile::tempdir().unwrap();
    let tools = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let result = support::bounded_output(Command::new("python3").args(["-c", r#"
import json, os, pathlib, subprocess, sys
sys.path.insert(0, sys.argv[1])
from compiler_workspace_wrapper import MARKER, effective_workspace_wrapper, workspace_wrapper
root = pathlib.Path(sys.argv[2]).resolve()
real = root / 'real/wrapper.py'
real.parent.mkdir()
real.write_text('#!/usr/bin/env python3\nimport pathlib, sys\nassert (pathlib.Path(sys.argv[0]).parent / "settings").exists(), sys.argv[0]\n')
real.chmod(0o700)
linked = root / 'linked'
linked.mkdir()
(linked / 'settings').write_text('')
(linked / 'wrapper').symlink_to(real)
config = root / '.cargo'
config.mkdir()
(config / 'config.toml').write_text("[build]\nrustc-workspace-wrapper='./linked/wrapper'\n")
os.environ['CARGO_HOME'] = str(root / 'cargo-home')
os.chdir(root)
inner = effective_workspace_wrapper([])
assert inner == str(linked / 'wrapper'), inner
target = workspace_wrapper(root / 'output', inner)
record = target.read_text().splitlines()[1].split(MARKER, 1)[1]
assert json.loads(record) == str(linked / 'wrapper'), record
subprocess.run([str(target)], check=True, timeout=3)
"#]).arg(tools).arg(temp.path())
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
