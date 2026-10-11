//! Exercises compiler launch and Clippy isolation through the comparison preparation boundary.
use super::*;
#[path = "../../tests/support/mod.rs"]
mod support;

#[test]
fn actual_go_failure_has_complete_comparison_evidence() {
    if !Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|result| result.status.success())
    {
        eprintln!("missing fixture: Go; skipping command compatibility test");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("go.mod"),
        "module fixture.invalid/evidence\n\ngo 1.23\n",
    )
    .unwrap();
    fs::write(temp.path().join("failure_test.go"), "package evidence\nimport \"testing\"\nfunc TestOld(t *testing.T) { t.Fatal(\"existing failure\") }\n").unwrap();
    let result = Command::new("go")
        .args(["test", "-json", "./..."])
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        signatures(
            &CommandSpec::new("go", vec!["test".into()]),
            &result,
            None,
            temp.path(),
            "architecture"
        )
        .unwrap()
        .is_some(),
        "unclassified real Go output: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn actual_clippy_failure_has_complete_comparison_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname='failure-evidence-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n").unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "#[deprecated] pub fn old() {} pub fn call() { old(); }",
    )
    .unwrap();
    let mut command = CommandSpec::cargo(&["clippy", "--offline", "--", "-D", "deprecated"]);
    command.env.push((
        "CARGO_BUILD_BUILD_DIR".into(),
        root.join("build").display().to_string(),
    ));
    let (command, _) = prepare(&command, &root, &root, "fixture").unwrap();
    let result = output(&command, &root).unwrap();
    assert!(!result.status.success());
    assert!(
        signatures(&command, &result, None, &root, "architecture")
            .unwrap()
            .is_some(),
        "unclassified real Clippy output: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[cfg(unix)]
#[test]
fn comparison_wrapper_preserves_hyphenated_environment() {
    let temp = tempfile::tempdir().unwrap();
    let mut command = CommandSpec::cargo(&["check"]);
    isolate_workspace_artifacts(&mut command, temp.path()).unwrap();
    let wrapper = &command.env.last().unwrap().1;
    let result = support::bounded_output(
        Command::new(wrapper)
            .args([
                "python3",
                "-c",
                "import os; assert os.environ.get('CARGO_BIN_EXE_test-client') == 'kept'",
            ])
            .envs(command.env.iter().map(|(k, v)| (k, v)))
            .env("CARGO_BIN_EXE_test-client", "kept"),
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn comparison_clippy_reads_head_and_base_sources() {
    let temp = tempfile::tempdir().unwrap();
    let mut command = CommandSpec::cargo(&["clippy", "--offline"]);
    command.env.push((
        "CARGO_BUILD_BUILD_DIR".into(),
        temp.path().join("build").display().to_string(),
    ));
    command
        .env
        .push(("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER".into(), String::new()));
    command
        .env
        .push(("RUSTC_WORKSPACE_WRAPPER".into(), String::new()));
    let mut artifacts = Vec::new();
    for name in ["head", "base"] {
        let root = temp.path().join(name);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname='comparison-clippy-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n").unwrap();
        let source = root.join("src/lib.rs");
        fs::write(
            &source,
            format!(
                "#[deprecated(note=\"{name}-only\")] pub fn old() {{}} pub fn call() {{ old(); }}"
            ),
        )
        .unwrap();
        fs::File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
            )
            .unwrap();
        let (prepared, _) = prepare(&command, &root, temp.path(), name).unwrap();
        let result = output(&prepared, &root).unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let messages = String::from_utf8_lossy(&result.stdout);
        for line in messages.lines() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if value["reason"] == "compiler-artifact" {
                artifacts.push(value["filenames"].clone());
            }
        }
        assert!(
            messages.contains(&format!("{name}-only")),
            "wrong checkout: {messages}"
        );
        if name == "base" {
            assert!(
                !messages.contains("head-only"),
                "reused head diagnostics: {messages}"
            );
        }
    }
    assert_eq!(artifacts.len(), 2);
    assert_ne!(
        artifacts[0], artifacts[1],
        "Clippy overwrote the other checkout artifact"
    );
}

#[cfg(unix)]
#[test]
fn comparison_wrappers_unwrap_two_and_three_levels() {
    use std::os::unix::fs::PermissionsExt;
    for levels in [2, 3] {
        for same_checkout in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let inner = temp.path().join("real inner wrapper");
            fs::write(&inner, "#!/usr/bin/env python3\nimport os, sys\nassert 'FIXTURE_INNER_CALLED' not in os.environ\nos.environ['FIXTURE_INNER_CALLED'] = 'once'\nos.execvp(sys.argv[1], sys.argv[1:])\n").unwrap();
            fs::set_permissions(&inner, fs::Permissions::from_mode(0o755)).unwrap();
            let mut command = CommandSpec::cargo(&["check"]);
            command.env.push((
                "RUSTC_WORKSPACE_WRAPPER".into(),
                inner.display().to_string(),
            ));
            for level in 0..levels {
                let root = temp.path().join(if same_checkout {
                    "same".into()
                } else {
                    format!("level-{level}")
                });
                isolate_workspace_artifacts(&mut command, &root).unwrap();
            }
            let wrapper = &command.env.last().unwrap().1;
            let result = support::bounded_output(Command::new(wrapper)
                .args(["python3", "-c", "import os; assert os.environ['CARGO_BIN_EXE_test-client'] == 'kept'; assert os.environ['FIXTURE_INNER_CALLED'] == 'once'"])
                .envs(command.env.iter().map(|(k,v)| (k,v)))
                .env("CARGO_BIN_EXE_test-client", "kept"));
            assert!(
                result.status.success(),
                "levels={levels}, same={same_checkout}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn mixed_workspace_wrappers_unwrap_each_other() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let inner = temp.path().join("real inner wrapper");
    fs::write(&inner, "#!/usr/bin/env python3\nimport os, sys\nassert 'FIXTURE_INNER_CALLED' not in os.environ\nos.environ['FIXTURE_INNER_CALLED'] = 'once'\nos.execvp(sys.argv[1], sys.argv[1:])\n").unwrap();
    fs::set_permissions(&inner, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = CommandSpec::cargo(&["check"]);
    command.env.push((
        "RUSTC_WORKSPACE_WRAPPER".into(),
        inner.display().to_string(),
    ));
    isolate_workspace_artifacts(&mut command, temp.path()).unwrap();
    let fake = temp.path().join("fake-cargo");
    fs::write(
        &fake,
        "#!/usr/bin/env python3\nimport os\nprint(os.environ['RUSTC_WORKSPACE_WRAPPER'])\n",
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let cargo_free = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cargo-free");
    let result = support::bounded_output(
        Command::new(cargo_free)
            .current_dir(temp.path())
            .envs(command.env.iter().map(|(k, v)| (k, v)))
            .env("CARGO_FREE_CARGO", fake)
            .env("CARGO_FREE_BUILD_ROOT", temp.path().join("build")),
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    command.env.push((
        "RUSTC_WORKSPACE_WRAPPER".into(),
        String::from_utf8(result.stdout).unwrap().trim().into(),
    ));
    isolate_workspace_artifacts(&mut command, temp.path()).unwrap();
    let result = support::bounded_output(
        Command::new(&command.env.last().unwrap().1)
            .args([
                "python3",
                "-c",
                "import os; assert os.environ['FIXTURE_INNER_CALLED'] == 'once'",
            ])
            .envs(command.env.iter().map(|(k, v)| (k, v))),
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
