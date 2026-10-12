use super::*;
use crate::local_worlds::test_support::scratch;
#[cfg(unix)]
use crate::local_worlds::test_support::{FAKE_DOCKER, write_script};

const LINUX: Platform = Platform {
    os: "linux",
    arch: "amd64",
};
const MAC: Platform = Platform {
    os: "darwin",
    arch: "arm64",
};

#[test]
fn native_hosts_never_probe_docker() {
    for platform in [
        LINUX,
        Platform {
            os: "windows",
            arch: "amd64",
        },
    ] {
        let info = detect_runtime(platform, "definitely-not-docker", &[]);
        assert_eq!(info.kind, RuntimeKind::Native);
    }
    let missing = detect_runtime(MAC, "/nonexistent/definitely-not-docker", &[]);
    assert_eq!(
        (missing.kind, missing.unavailable),
        (RuntimeKind::None, Some(UnavailableReason::DockerMissing))
    );
}

/// `docker info` decides between the container runtime and Dragonfly.
#[cfg(unix)]
#[test]
fn the_docker_probe_picks_the_container_or_reports_it_stopped() {
    let dir = scratch();
    let docker = dir.path().join("docker");
    write_script(&docker, FAKE_DOCKER);
    let docker = docker.to_string_lossy().into_owned();
    let log: (OsString, OsString) = ("FAKE_DOCKER_LOG".into(), dir.path().join("log").into());
    let up = detect_runtime(MAC, &docker, std::slice::from_ref(&log));
    assert_eq!((up.kind, up.unavailable), (RuntimeKind::Container, None));
    let down = detect_runtime(
        MAC,
        &docker,
        &[log, ("FAKE_DOCKER_DOWN".into(), "1".into())],
    );
    assert_eq!(
        (down.kind, down.unavailable),
        (RuntimeKind::None, Some(UnavailableReason::DockerNotRunning))
    );
}

/// A Finder-launched app's PATH has no Docker folder, so the CLI is found in its install folders.
#[cfg(unix)]
#[test]
fn docker_is_found_in_install_folders_outside_path() {
    let home = scratch();
    let dirs = docker_dirs(Some(home.path()));
    for expected in [
        home.path().join(".orbstack/bin"),
        home.path().join(".docker/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/Applications/Docker.app/Contents/Resources/bin"),
    ] {
        assert!(
            dirs.contains(&expected),
            "{dirs:?} lacks {}",
            expected.display()
        );
    }
    let not_executable = home.path().join("a");
    std::fs::create_dir_all(&not_executable).expect("folder");
    std::fs::write(not_executable.join("docker"), b"#!/bin/sh\n").expect("plain file");
    let orbstack = home.path().join(".orbstack/bin");
    std::fs::create_dir_all(&orbstack).expect("folder");
    write_script(&orbstack.join("docker"), b"#!/bin/sh\n");
    let empty_path = home.path().join("empty");
    let found = find_docker(
        "docker",
        Some(empty_path.as_os_str()),
        &[
            home.path().join("missing"),
            not_executable.clone(),
            orbstack.clone(),
        ],
    );
    assert_eq!(found, Some(orbstack.join("docker")));
    assert_eq!(
        find_docker("docker", Some(empty_path.as_os_str()), &[not_executable]),
        None,
        "a file without execute permission is not docker"
    );
    let explicit = home.path().join("nope/docker");
    assert_eq!(
        find_docker(&explicit.to_string_lossy(), None, &[orbstack]),
        None,
        "an explicit path never falls back to the install folders"
    );
}

/// Docker runs credential helpers beside it, so its folder leads the child's PATH.
#[test]
fn docker_runs_by_absolute_path_with_its_folder_first_on_path() {
    let folder = scratch();
    let docker = folder.path().join("docker");
    let command = command_for(
        Some(docker.clone()),
        "docker",
        &[],
        Some(OsString::from("/nonexistent")),
    );
    assert_eq!(Path::new(command.get_program()), docker.as_path());
    let path = command
        .get_envs()
        .find(|(key, _)| *key == "PATH")
        .and_then(|(_, value)| value)
        .expect("PATH");
    let separator = if cfg!(windows) { ";" } else { ":" };
    let expected = format!("{}{separator}/nonexistent", folder.path().display());
    assert_eq!(path.to_string_lossy(), expected);
    let unresolved = command_for(None, "docker", &[], None);
    assert_eq!(unresolved.get_program(), "docker");
    assert!(unresolved.get_envs().all(|(key, _)| key != "PATH"));
    let env = [("PATH".into(), "/override".into())];
    assert_eq!(env_value(&env, "PATH"), Some(OsString::from("/override")));
}
