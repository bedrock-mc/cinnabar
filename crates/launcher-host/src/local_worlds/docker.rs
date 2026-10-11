use std::{
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use launcher::local_worlds::world::UnavailableReason;

use super::{
    Cancel,
    provision::{Platform, RuntimeInfo, RuntimeKind},
};

/// The Docker-compatible CLI's default name.
pub(crate) const DOCKER: &str = "docker";
/// Bounds `docker info`, which hangs while a daemon starts.
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(25);

/// Where Docker Desktop, Homebrew and OrbStack put the CLI; a Finder-launched app's PATH has none.
pub(crate) fn docker_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
    ];
    if let Some(home) = home {
        dirs.push(home.join(".docker").join("bin"));
    }
    dirs.push(PathBuf::from(
        "/Applications/Docker.app/Contents/Resources/bin",
    ));
    if let Some(home) = home {
        dirs.push(home.join(".orbstack").join("bin"));
    }
    dirs
}

/// The CLI's absolute path from `path`, else from `dirs`; a name with a separator is used as given.
pub(crate) fn find_docker(name: &str, path: Option<&OsStr>, dirs: &[PathBuf]) -> Option<PathBuf> {
    if name.contains(['/', '\\']) {
        let given = Path::new(name);
        return executable(given)
            .then(|| std::path::absolute(given).unwrap_or_else(|_| given.to_path_buf()));
    }
    let searched = path
        .map(|path| env::split_paths(path).collect::<Vec<_>>())
        .unwrap_or_default();
    searched
        .iter()
        .map(|dir| {
            if dir.as_os_str().is_empty() {
                Path::new(".")
            } else {
                dir.as_path()
            }
        })
        .chain(dirs.iter().map(PathBuf::as_path))
        .flat_map(|dir| candidates(&dir.join(name)))
        .find(|candidate| executable(candidate))
        .map(|candidate| std::path::absolute(&candidate).unwrap_or(candidate))
}

/// `path` itself, plus its executable extensions on Windows.
fn candidates(path: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if cfg!(windows) && path.extension().is_none() {
        for extension in ["exe", "cmd", "bat", "com"] {
            candidates.push(path.with_extension(extension));
        }
    }
    candidates.push(path.to_path_buf());
    candidates
}

fn executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && runnable(&metadata))
}

#[cfg(unix)]
fn runnable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn runnable(_metadata: &std::fs::Metadata) -> bool {
    true
}

fn home_dir() -> Option<PathBuf> {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

fn lookup_docker(name: &str) -> Option<PathBuf> {
    find_docker(
        name,
        env::var_os("PATH").as_deref(),
        &docker_dirs(home_dir().as_deref()),
    )
}

/// Runs the Docker CLI by absolute path with its folder first on PATH, where its credential
/// helpers live; `env` is added to the inherited environment.
pub(crate) fn docker_command(docker: &str, env: &[(OsString, OsString)]) -> Command {
    command_for(lookup_docker(docker), docker, env, env::var_os("PATH"))
}

pub(crate) fn command_for(
    found: Option<PathBuf>,
    docker: &str,
    env: &[(OsString, OsString)],
    inherited_path: Option<OsString>,
) -> Command {
    let mut command = Command::new(found.as_deref().unwrap_or(Path::new(docker)));
    command.envs(env.iter().map(|(key, value)| (key, value)));
    if let Some(folder) = found.as_deref().and_then(Path::parent) {
        let path = env_value(env, "PATH").or(inherited_path);
        command.env("PATH", prepend_path(folder, path.as_deref()));
    }
    command
}

fn prepend_path(folder: &Path, path: Option<&OsStr>) -> OsString {
    let mut joined = folder.as_os_str().to_owned();
    if let Some(path) = path.filter(|path| !path.is_empty()) {
        joined.push(if cfg!(windows) { ";" } else { ":" });
        joined.push(path);
    }
    joined
}

/// The last value of `key` in `env`, as process spawning resolves duplicates; Windows names are
/// case-insensitive.
pub(crate) fn env_value(env: &[(OsString, OsString)], key: &str) -> Option<OsString> {
    env.iter()
        .rev()
        .find(|(name, _)| {
            name == key || (cfg!(windows) && name.to_string_lossy().eq_ignore_ascii_case(key))
        })
        .map(|(_, value)| value.clone())
}

/// Runs `command` quietly, killing it once `timeout` passes; true when it exited successfully.
pub(crate) fn succeeds(command: Command, timeout: Duration) -> bool {
    finishes(command, timeout, &Cancel::never()) == Some(true)
}

/// Runs `command` quietly, killing it once `timeout` passes or `cancel` fires; `None` when
/// cancelled, else whether it exited successfully.
pub(crate) fn finishes(mut command: Command, timeout: Duration, cancel: &Cancel) -> Option<bool> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let Ok(mut child) = command.spawn() else {
        return Some(false);
    };
    let deadline = Instant::now() + timeout;
    loop {
        let cancelled = cancel.is_cancelled();
        match child.try_wait() {
            Ok(Some(status)) => return Some(status.success()),
            Ok(None) if !cancelled && Instant::now() < deadline => thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return (!cancelled).then_some(false);
            }
        }
    }
}

/// Prefers the native server, then a Docker-compatible container (probed with `docker info`).
pub(crate) fn detect_runtime(
    platform: Platform,
    docker: &str,
    env: &[(OsString, OsString)],
) -> RuntimeInfo {
    if platform.runs_bds() {
        return RuntimeInfo::native();
    }
    if lookup_docker(docker).is_none() {
        return RuntimeInfo::none(
            "no native Bedrock Dedicated Server for this platform and Docker is not installed; new worlds use dragonfly",
            Some(UnavailableReason::DockerMissing),
        );
    }
    let mut info = docker_command(docker, env);
    info.arg("info");
    if succeeds(info, PROBE_TIMEOUT) {
        return RuntimeInfo {
            kind: RuntimeKind::Container,
            reason: "no native Bedrock Dedicated Server for this platform; running the Linux build in a container".to_owned(),
            unavailable: None,
        };
    }
    RuntimeInfo::none(
        "no native Bedrock Dedicated Server for this platform and Docker is not running; new worlds use dragonfly",
        Some(UnavailableReason::DockerNotRunning),
    )
}

#[cfg(test)]
#[path = "docker_tests.rs"]
mod tests;
