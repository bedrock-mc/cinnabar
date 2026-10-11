//! Tests shared build caches with small offline packages and lock ownership with fake Cargo.
mod support;
#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt;
    use std::{
        fs,
        process::{Command, Stdio},
    };

    #[test]
    fn launcher_and_compiler_wrapper_preserve_hyphenated_environment() {
        let temp = tempfile::tempdir().unwrap();
        let fake = temp.path().join("fake-cargo");
        fs::write(&fake, r#"#!/usr/bin/env python3
import os, subprocess, sys
assert os.environ.get('CARGO_BIN_EXE_test-client') == 'kept', 'launcher lost variable'
subprocess.run([os.environ['RUSTC_WORKSPACE_WRAPPER'], sys.executable, '-c', "import os; assert os.environ.get('CARGO_BIN_EXE_test-client') == 'kept', 'compiler lost variable'"], check=True)
"#).unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
        let wrapper = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../cargo-free");
        let result = Command::new(wrapper)
            .current_dir(temp.path())
            .env("CARGO_FREE_CARGO", fake)
            .env("CARGO_FREE_BUILD_ROOT", temp.path().join("build"))
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env_remove("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER")
            .env("CARGO_BIN_EXE_test-client", "kept")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn clippy_reads_each_worktrees_source() {
        let temp = tempfile::tempdir().unwrap();
        let wrapper = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../cargo-free");
        let mut artifacts = Vec::new();
        for name in ["first", "other"] {
            let root = temp.path().join(name);
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(
                root.join("Cargo.toml"),
                "[package]\nname='clippy-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n",
            )
            .unwrap();
            let source = root.join("src/lib.rs");
            fs::write(&source, format!("#[deprecated(note=\"{name}-only\")] pub fn old() {{}} pub fn call() {{ old(); }}")).unwrap();
            fs::File::options()
                .write(true)
                .open(&source)
                .unwrap()
                .set_times(
                    std::fs::FileTimes::new()
                        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
                )
                .unwrap();
            let result = Command::new(&wrapper)
                .current_dir(&root)
                .env_remove("CARGO_TARGET_DIR")
                .env_remove("CARGO_BUILD_TARGET_DIR")
                .env_remove("RUSTC_WORKSPACE_WRAPPER")
                .env_remove("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER")
                .env("CARGO_FREE_BUILD_ROOT", temp.path().join("build"))
                .args(["clippy", "--offline", "--message-format=json"])
                .output()
                .unwrap();
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
            if name == "other" {
                assert!(
                    !messages.contains("first-only"),
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

    #[test]
    fn sequential_worktrees_isolate_workspace_crates_and_share_dependencies() {
        let temp = tempfile::tempdir().unwrap();
        let dep = temp.path().join("dependency");
        fs::create_dir_all(dep.join("src")).unwrap();
        fs::write(
            dep.join("Cargo.toml"),
            "[package]\nname='fixture-dependency'\nversion='0.1.0'\nedition='2024'\n",
        )
        .unwrap();
        fs::write(dep.join("src/lib.rs"), "pub fn value() -> u8 { 7 }").unwrap();
        let inner = temp.path().join("existing wrapper");
        let calls = temp.path().join("wrapper-calls");
        fs::write(
            &inner,
            format!(
                "#!/bin/sh\nprintf called >> '{}'\nexec \"$@\"\n",
                calls.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&inner, fs::Permissions::from_mode(0o755)).unwrap();
        let wrapper = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../cargo-free");
        let mut dependency_artifacts = Vec::new();
        let mut workspace_artifacts = Vec::new();
        for name in ["first", "other"] {
            let root = temp.path().join(name);
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(root.join("Cargo.toml"), format!("[package]\nname='worktree-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n[dependencies]\nfixture-dependency={{path={:?}}}\n", dep)).unwrap();
            let source = root.join("src/main.rs");
            fs::write(
                &source,
                format!("fn main() {{ println!(\"{name} {{}}\", fixture_dependency::value()); }}"),
            )
            .unwrap();
            // Both trees predate the first build, as older commits do in linked worktrees.
            fs::File::options()
                .write(true)
                .open(&source)
                .unwrap()
                .set_times(
                    std::fs::FileTimes::new()
                        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
                )
                .unwrap();
            let output = Command::new(&wrapper)
                .current_dir(&root)
                .env_remove("CARGO_TARGET_DIR")
                .env_remove("CARGO_BUILD_TARGET_DIR")
                .env_remove("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER")
                .env("RUSTC_WORKSPACE_WRAPPER", &inner)
                .env("CARGO_FREE_BUILD_ROOT", temp.path().join("build"))
                .args(["build", "--offline", "--message-format=json"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                if value["reason"] != "compiler-artifact" {
                    continue;
                }
                if value["target"]["name"] == "fixture_dependency" {
                    dependency_artifacts.push(value["filenames"][0].as_str().unwrap().to_owned());
                } else if value["target"]["name"] == "worktree-fixture" {
                    workspace_artifacts.push(value["filenames"][0].as_str().unwrap().to_owned());
                    let run = Command::new(value["executable"].as_str().unwrap())
                        .output()
                        .unwrap();
                    assert!(run.status.success());
                    assert_eq!(
                        String::from_utf8_lossy(&run.stdout).trim(),
                        format!("{name} 7")
                    );
                }
            }
        }
        assert_eq!(dependency_artifacts.len(), 2);
        assert_eq!(dependency_artifacts[0], dependency_artifacts[1]);
        assert_eq!(workspace_artifacts.len(), 2);
        assert_ne!(workspace_artifacts[0], workspace_artifacts[1]);
        assert!(!fs::read_to_string(calls).unwrap().is_empty());
    }

    #[test]
    fn occupied_slots_do_not_block_other_worktrees_and_locks_survive_exec() {
        let temp = tempfile::tempdir().unwrap();
        let fake = temp.path().join("fake-cargo");
        fs::write(
            &fake,
            "#!/bin/sh\nprintf '%s\\n' \"$CARGO_BUILD_BUILD_DIR\"\nread reply\n",
        )
        .unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
        let wrapper = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../cargo-free");
        let spawn = || {
            Command::new(&wrapper)
                .env("CARGO_FREE_BUILD_ROOT", temp.path().join("build"))
                .env("CARGO_FREE_CARGO", &fake)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap()
        };
        let mut children = Vec::new();
        for name in ["cinnabar", "cinnabar-2", "cinnabar-3", "cinnabar"] {
            let mut child = spawn();
            let mut line = String::new();
            use std::io::BufRead;
            std::io::BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut line)
                .unwrap();
            assert_eq!(
                line.trim(),
                temp.path().join("build").join(name).to_str().unwrap()
            );
            children.push(child);
        }
        for mut child in children {
            child.kill().unwrap();
            child.wait().unwrap();
        }
        let mut child = spawn();
        let mut line = String::new();
        use std::io::BufRead;
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(
            line.trim(),
            temp.path().join("build/cinnabar").to_str().unwrap()
        );
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

#[cfg(unix)]
#[test]
fn cargo_free_unwraps_two_and_three_levels() {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};
    let launcher = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cargo-free")
        .canonicalize()
        .unwrap();
    for levels in [2, 3] {
        for same_checkout in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            assert!(
                Command::new("git")
                    .args(["init", "--quiet"])
                    .arg(temp.path())
                    .status()
                    .unwrap()
                    .success()
            );
            let fake = temp.path().join("fake-cargo");
            fs::write(&fake, r#"#!/usr/bin/env python3
import os, pathlib, subprocess, sys
level = int(os.environ.get('FIXTURE_LEVEL', '1'))
if level < int(os.environ['FIXTURE_LEVELS']):
    os.environ['FIXTURE_LEVEL'] = str(level + 1)
    if os.environ['FIXTURE_SAME'] == 'false':
        root = pathlib.Path(os.environ['FIXTURE_ROOT']) / str(level)
        root.mkdir()
        subprocess.run(['git', 'init', '--quiet', str(root)], check=True, timeout=3)
        os.chdir(root)
    os.execv(os.environ['FIXTURE_LAUNCHER'], [os.environ['FIXTURE_LAUNCHER']])
os.execv(os.environ['RUSTC_WORKSPACE_WRAPPER'], [os.environ['RUSTC_WORKSPACE_WRAPPER'], sys.executable, '-c', "import os; assert os.environ['CARGO_BIN_EXE_test-client'] == 'kept'; assert os.environ['FIXTURE_INNER_CALLED'] == 'once'"])
"#).unwrap();
            let inner = temp.path().join("real inner wrapper");
            fs::write(&inner, "#!/usr/bin/env python3\nimport os, sys\nassert 'FIXTURE_INNER_CALLED' not in os.environ\nos.environ['FIXTURE_INNER_CALLED'] = 'once'\nos.execvp(sys.argv[1], sys.argv[1:])\n").unwrap();
            for path in [&fake, &inner] {
                fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
            }
            let result = support::bounded_output(
                Command::new(&launcher)
                    .current_dir(temp.path())
                    .env("CARGO_FREE_CARGO", &fake)
                    .env("CARGO_FREE_BUILD_ROOT", temp.path().join("build"))
                    .env("RUSTC_WORKSPACE_WRAPPER", &inner)
                    .env("FIXTURE_LEVELS", levels.to_string())
                    .env("FIXTURE_SAME", same_checkout.to_string())
                    .env("FIXTURE_ROOT", temp.path())
                    .env("FIXTURE_LAUNCHER", &launcher)
                    .env("CARGO_BIN_EXE_test-client", "kept"),
            );
            assert!(
                result.status.success(),
                "levels={levels}, same={same_checkout}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}
