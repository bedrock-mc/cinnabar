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
    for package in ["failing", "passing", "cached"] {
        let dir = temp.path().join(package);
        fs::create_dir(&dir).unwrap();
        let body = if package == "failing" {
            "t.Fatal(\"existing failure\")"
        } else {
            ""
        };
        fs::write(
            dir.join("fixture_test.go"),
            format!(
                "package evidence\nimport \"testing\"\nfunc TestOld(t *testing.T) {{ {body} }}\n"
            ),
        )
        .unwrap();
    }
    let warm = Command::new("go")
        .args(["test", "-p", "2", "-json", "./cached"])
        .current_dir(temp.path())
        .env("GOCACHE", temp.path().join("go-cache"))
        .env("GOMAXPROCS", "4")
        .output()
        .unwrap();
    assert!(
        warm.status.success(),
        "{}",
        String::from_utf8_lossy(&warm.stderr)
    );
    let result = Command::new("go")
        .args(["test", "-p", "2", "-json", "./..."])
        .current_dir(temp.path())
        .env("GOCACHE", temp.path().join("go-cache"))
        .env("GOMAXPROCS", "4")
        .output()
        .unwrap();
    assert!(!result.status.success());
    let events: Vec<serde_json::Value> = String::from_utf8_lossy(&result.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(events.iter().any(|event| event["Output"] == "PASS\n"));
    assert!(events.iter().any(|event| {
        event["Output"]
            .as_str()
            .is_some_and(|line| line.starts_with("ok") && line.contains("(cached)"))
    }));
    assert_eq!(
        signatures(
            &CommandSpec::new("go", vec!["test".into()]),
            &result,
            None,
            temp.path(),
            "architecture"
        )
        .unwrap(),
        Some(BTreeSet::from([
            "fixture.invalid/evidence/failing\tTestOld".to_owned()
        ])),
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
        "#!/usr/bin/env python3\nimport os,sys,json\nif 'metadata' in sys.argv:\n    print(json.dumps({'workspace_root':os.getcwd()})); sys.exit(0)\nprint(os.environ['RUSTC_WORKSPACE_WRAPPER'])\n",
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

#[test]
fn clippy_compilation_errors_are_not_comparable_lints() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("benches")).unwrap();
    fs::write(temp.path().join("Cargo.toml"), "[package]\nname='compile-error-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n[[bench]]\nname='broken'\nharness=false\n").unwrap();
    fs::write(
        temp.path().join("benches/broken.rs"),
        "fn main() { missing_function(); }",
    )
    .unwrap();
    let mut command =
        CommandSpec::cargo(&["clippy", "--offline", "--benches", "--message-format=json"]);
    command.env.push((
        "CARGO_BUILD_BUILD_DIR".into(),
        temp.path().join("build").display().to_string(),
    ));
    command.env.push((
        "CARGO_TARGET_DIR".into(),
        temp.path().join("target").display().to_string(),
    ));
    let result = output(&command, temp.path()).unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("E0425"));
    assert!(
        signatures(&command, &result, None, temp.path(), "architecture")
            .unwrap()
            .is_none(),
        "waived an incomplete compilation"
    );
}

/// Runs a real failed unit test and optional doctest through the comparison output boundary.
fn failed_libtest_fixture(
    source: &str,
    lib_only: bool,
) -> (tempfile::TempDir, CommandSpec, Output) {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='libtest-output-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n",
    )
    .unwrap();
    fs::write(temp.path().join("src/lib.rs"), source).unwrap();
    let mut command = CommandSpec::cargo(&[
        "test",
        "--offline",
        "--no-fail-fast",
        "--message-format=json",
    ]);
    if lib_only {
        command.args.push("--lib".into());
    }
    command.env.push((
        "CARGO_BUILD_BUILD_DIR".into(),
        temp.path().join("build").display().to_string(),
    ));
    command.env.push((
        "CARGO_TARGET_DIR".into(),
        temp.path().join("target").display().to_string(),
    ));
    let result = output(&command, temp.path()).unwrap();
    assert!(!result.status.success());
    (temp, command, result)
}

#[test]
fn passing_doctest_sections_preserve_unit_failure_comparison() {
    for docs in ["", "/// ```\n/// assert_eq!(2 + 2, 4);\n/// ```\n"] {
        let (root, command, result) = failed_libtest_fixture(
            &format!(
                "{docs}pub fn documented() {{}}\n#[test] fn existing() {{ panic!(\"existing failure\"); }}"
            ),
            false,
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("Doc-tests"));
        let failures = signatures(&command, &result, None, root.path(), "architecture")
            .unwrap()
            .unwrap_or_else(|| {
                panic!(
                    "passing doctests blocked comparison: {}\n{}",
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                )
            });
        assert_eq!(failures.len(), 1);
    }
    let (root, command, result) = failed_libtest_fixture(
        "/// ```\n/// panic!(\"doctest failure\");\n/// ```\npub fn documented() {}\n#[test] fn existing() { panic!(\"existing failure\"); }",
        false,
    );
    assert!(
        signatures(&command, &result, None, root.path(), "architecture")
            .unwrap()
            .is_none(),
        "waived a failed doctest"
    );
}

#[test]
fn failed_test_json_is_captured_output_instead_of_cargo_events() {
    for json in ["[1,2]", "{\"reason\":\"unrecognized-failure\"}"] {
        let source = format!(
            "#[test] fn existing() {{ println!(\"{{}}\", {json:?}); panic!(\"existing failure\"); }}"
        );
        let (root, command, result) = failed_libtest_fixture(&source, true);
        let failures = signatures(&command, &result, None, root.path(), "architecture")
            .unwrap()
            .expect("captured JSON blocked comparison");
        assert_eq!(failures.len(), 1);
        let mut outside = result.clone();
        outside
            .stdout
            .extend_from_slice(format!("\n{json}\n").as_bytes());
        assert!(
            signatures(&command, &outside, None, root.path(), "architecture")
                .unwrap()
                .is_none(),
            "accepted unknown output outside a test body"
        );
    }
}

#[cfg(unix)]
#[test]
fn configured_workspace_wrappers_survive_both_isolation_paths() {
    use std::os::unix::fs::PermissionsExt;
    const CHILD: &str = "CARGO_CONFIG_WRAPPER_FIXTURE";
    if std::env::var_os(CHILD).is_none() {
        let result = support::bounded_output(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "baseline::workspace_tests::configured_workspace_wrappers_survive_both_isolation_paths", "--nocapture"])
                .env(CHILD, "1")
                .env_remove("RUSTC_WORKSPACE_WRAPPER")
                .env_remove("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"),
        );
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir(root.join(".cargo")).unwrap();
    fs::write(root.join("Cargo.toml"), "[package]\nname='configured-wrapper-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n").unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\nrustc-workspace-wrapper='./configured-wrapper'\n",
    )
    .unwrap();
    let inner = root.join("configured-wrapper");
    fs::write(&inner, "#!/usr/bin/env python3\nimport pathlib,sys,os\nwith pathlib.Path(__file__).with_name('calls').open('a') as calls: calls.write(' '.join(sys.argv[1:]) + '\\n')\nos.execvp(sys.argv[1], sys.argv[1:])\n").unwrap();
    fs::set_permissions(&inner, fs::Permissions::from_mode(0o755)).unwrap();
    let launcher = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cargo-free");
    for layout in ["file", "symlink"] {
        if layout == "symlink" {
            let shared = root.join("shared/settings/config.toml");
            fs::create_dir_all(shared.parent().unwrap()).unwrap();
            fs::rename(root.join(".cargo/config.toml"), &shared).unwrap();
            std::os::unix::fs::symlink(&shared, root.join(".cargo/config.toml")).unwrap();
        }
        for mode in ["normal", "free", "comparison"] {
            let calls = root.join("calls");
            fs::remove_file(&calls).ok();
            let mut command = CommandSpec::cargo(&["check", "--offline"]);
            command.env.push((
                "CARGO_BUILD_BUILD_DIR".into(),
                root.join(format!("{layout}-{mode}")).display().to_string(),
            ));
            command.env.push((
                "CARGO_TARGET_DIR".into(),
                root.join(format!("target-{layout}-{mode}"))
                    .display()
                    .to_string(),
            ));
            if mode == "comparison" {
                isolate_workspace_artifacts(&mut command, root).unwrap();
            } else if mode == "free" {
                command.program = launcher.display().to_string();
                command.env.push((
                    "CARGO_FREE_BUILD_ROOT".into(),
                    root.join(format!("free-build-{layout}"))
                        .display()
                        .to_string(),
                ));
            }
            let result = output(&command, root).unwrap();
            assert!(
                result.status.success(),
                "{mode}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(
                fs::read_to_string(&calls)
                    .unwrap_or_default()
                    .contains("src/main.rs"),
                "{mode} bypassed the configured workspace wrapper"
            );
        }
    }
}

#[test]
fn base_package_selectors_accept_a_different_workspace_version() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let head = root.join("head space");
    let base = root.join("base space");
    for (root, version) in [(&head, "0.2.0"), (&base, "0.1.0")] {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[package]\nname='architecture'\nversion='{version}'\nedition='2024'\n[workspace]\n"
            ),
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "").unwrap();
        assert!(
            Command::new("cargo")
                .current_dir(root)
                .args(["generate-lockfile", "--offline"])
                .status()
                .unwrap()
                .success()
        );
    }
    let selector = format!("path+file://{}#architecture@0.2.0", url_path(&head));
    for arguments in [
        vec!["run", "-p", &selector],
        vec!["clippy", "--package", &selector],
        vec!["nextest", "run", "-p", &selector],
    ] {
        let remapped: Vec<_> = arguments
            .iter()
            .map(|argument| remap_argument(argument, &head, &base).unwrap())
            .collect();
        let selector = remapped.last().unwrap();
        let result = Command::new("cargo")
            .current_dir(&base)
            .args(["pkgid", "--offline", "-p", selector])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}: {}",
            remapped.join(" "),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("architecture@0.1.0"));
    }
}
