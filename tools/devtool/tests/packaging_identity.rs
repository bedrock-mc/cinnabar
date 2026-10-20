//! Exercises the build recipes without compiling the client or downloading assets.
#[cfg(unix)]
#[test]
fn development_and_packaging_builds_receive_the_selected_revision() {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().unwrap();
    let cargo = temp.path().join("fake-cargo");
    let received = temp.path().join("commit");
    fs::write(
        &cargo,
        "#!/bin/sh\nprintf '%s' \"${CINNABAR_BUILD_COMMIT-unset}\" > \"$FIXTURE_BUILD_COMMIT\"\n",
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let git = temp.path().join("git");
    fs::write(
        &git,
        "#!/bin/sh\n[ \"$1 $2\" = 'rev-parse HEAD' ] || exit 1\nprintf 'fixture-revision\\n'\n",
    )
    .unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    // The pre-rename variable still selects the revision for one release.
    for (variable, revision) in [
        (None, "fixture-revision"),
        (Some("CINNABAR_BUILD_COMMIT"), "selected-revision"),
        (Some("RUST_MCBE_BUILD_COMMIT"), "legacy-revision"),
    ] {
        for goal in ["play-build", "package-binaries"] {
            let mut make = Command::new("make");
            make.current_dir(&root)
                .args([
                    "-o",
                    "assets",
                    "-o",
                    "physics-assets",
                    goal,
                    "GO=true",
                    "DIST_TARGET=fixture",
                ])
                .arg(format!("CARGO={}", cargo.display()))
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        temp.path().display(),
                        std::env::var("PATH").unwrap()
                    ),
                )
                .env_remove("CINNABAR_BUILD_COMMIT")
                .env_remove("RUST_MCBE_BUILD_COMMIT")
                .env("FIXTURE_BUILD_COMMIT", &received);
            if let Some(variable) = variable {
                make.arg(format!("{variable}={revision}"));
            }
            let result = make.output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(fs::read_to_string(&received).unwrap(), revision, "{goal}");
        }
    }
}
