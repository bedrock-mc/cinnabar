//! Exercises complete failure classification through real comparison worktrees.
use super::*;

/// Checks only completely classified failures can be waived on either worktree.
#[cfg(unix)]
fn assert_unknown_output_is_strict(args: &[&str], unknown: &str, classified: bool) {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir(&root).unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "--quiet",
            "-m",
            "fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .current_dir(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let cargo = bin.join("cargo");
    fs::write(&cargo, r#"#!/usr/bin/env python3
import json, os, pathlib, sys
if sys.argv[1] == 'run':
    print('render: forbidden dependency path `render -> protocol`')
else:
    print(json.dumps({'reason':'compiler-message', 'message':{'level':'error', 'code':{'code':'clippy::fixture'}, 'message':'existing lint', 'spans':[], 'children':[], 'rendered':None}}))
    print('error: could not compile `fixture` (lib) due to 1 previous error', file=sys.stderr)
if 'FIXTURE_UNKNOWN' in os.environ:
    on_base = pathlib.Path.cwd().name == 'base'
    if on_base == ('FIXTURE_UNKNOWN_ON_BASE' in os.environ):
        print(os.environ['FIXTURE_UNKNOWN'], file=sys.stderr)
sys.exit(1)
"#).unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = if args.first() == Some(&"run") {
        let packages = [crate::Package::from_owned(
            format!(
                "path+file://{}/tools/architecture#{}",
                root.display(),
                env!("CARGO_PKG_VERSION")
            ),
            "architecture".into(),
            "tools/architecture".into(),
            vec![],
            false,
        )];
        crate::verification_commands(
            &crate::Selection::NoPackages,
            crate::TestRunner::Cargo,
            &packages,
        )
        .remove(1)
    } else {
        CommandSpec::cargo(args)
    };
    command.env.push((
        "PATH".into(),
        format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
    ));
    let mut comparison = Comparison {
        architecture_selector: if args.first() == Some(&"run") {
            command.args[2].clone()
        } else {
            "architecture".into()
        },
        base: "HEAD".into(),
        build_dir: temp.path().join("build").display().to_string(),
        root,
        scratch: tempfile::tempdir().unwrap(),
        worktree: None,
    };
    assert!(comparison.verify(command.clone()).is_ok());
    command.env.push(("FIXTURE_UNKNOWN".into(), unknown.into()));
    assert!(
        comparison.verify(command.clone()).is_err(),
        "waived unclassified head output"
    );
    command
        .env
        .push(("FIXTURE_UNKNOWN_ON_BASE".into(), "1".into()));
    assert_eq!(
        comparison.verify(command).is_ok(),
        classified,
        "base output classification did not control the waiver"
    );
}

#[cfg(unix)]
#[test]
fn existing_architecture_error_cannot_hide_a_new_crate_rule_failure() {
    assert_unknown_output_is_strict(
        &["run", "-p", "architecture"],
        "workspace member `crates/new` has no crate rule",
        true,
    );
}

#[cfg(unix)]
#[test]
fn existing_clippy_error_cannot_hide_a_new_build_script_failure() {
    assert_unknown_output_is_strict(
        &["clippy"],
        "error: failed to run custom build command for `fixture-build`",
        false,
    );
}

#[cfg(unix)]
#[test]
fn unknown_failure_lines_cannot_be_waived_by_subset_matching() {
    assert_unknown_output_is_strict(&["clippy"], "unclassified failure output", false);
}
