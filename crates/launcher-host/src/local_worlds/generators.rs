//! The process's fixed local server and its independently discovered generator catalog.

use launcher::local_worlds::generators::{GeneratorInfo, parse_catalog};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, OnceLock},
    thread,
    time::Duration,
};

/// A catalog resolves once; reading it never waits for discovery.
pub(super) type Catalog = Arc<OnceLock<Result<Vec<GeneratorInfo>, String>>>;

/// The executable selected at startup and the catalog shared by every manager.
pub(super) struct LocalServer {
    pub binary: PathBuf,
    pub catalog: Catalog,
}

/// Selects the server once per process and starts discovery on its own thread.
pub(super) fn local_server(shipped: PathBuf) -> &'static LocalServer {
    static SERVER: OnceLock<LocalServer> = OnceLock::new();
    SERVER.get_or_init(|| {
        let binary = server_binary(shipped);
        let catalog = start_discovery(binary.clone());
        LocalServer { binary, catalog }
    })
}

/// Starts a single discovery task; callers only inspect the returned cell with `get`.
pub(super) fn start_discovery(binary: PathBuf) -> Catalog {
    let catalog = Catalog::default();
    let result = Arc::clone(&catalog);
    if let Err(error) = thread::Builder::new()
        .name("local-world-generators".into())
        .spawn(move || {
            let discovered = discover(&binary);
            if let Err(detail) = &discovered {
                tracing::error!("local server generator discovery failed: {detail}");
            }
            let _ = result.set(discovered);
        })
    {
        let _ = catalog.set(Err(error.to_string()));
    }
    catalog
}

/// Runs catalog discovery without inheriting input or mixing diagnostics into the JSON.
fn discover(binary: &Path) -> Result<Vec<GeneratorInfo>, String> {
    let mut command = Command::new(binary);
    command
        .arg("-list-generators")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child =
        Arc::new(crate::lifecycle::children::spawn(&mut command).map_err(|e| e.to_string())?);
    let mut stdout = child.take_stdout().expect("piped catalog output");
    let (sent, received) = crossbeam_channel::bounded(1);
    let watched = Arc::clone(&child);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = stdout.read_to_end(&mut bytes);
        let status = watched.wait();
        let _ = sent.send((read, status, bytes));
    });
    let (read, status, bytes) = match received.recv_timeout(Duration::from_secs(30)) {
        Ok(output) => output,
        Err(_) => {
            child.kill();
            let _ = child.wait();
            return Err("-list-generators did not finish in time".into());
        }
    };
    read.map_err(|e| e.to_string())?;
    if !status.is_some_and(|status| status.success()) {
        return Err(format!("-list-generators failed: {status:?}"));
    }
    parse_catalog(&bytes)
}

/// Resolves an explicit executable override only for a development build. A relative override
/// is anchored to the working directory so discovery and startup run the same file, never a
/// `PATH` lookup.
fn select_binary(
    shipped: PathBuf,
    override_path: Option<std::ffi::OsString>,
    development: bool,
) -> PathBuf {
    if development && let Some(path) = override_path {
        let path = PathBuf::from(path);
        return std::path::absolute(&path).unwrap_or(path);
    }
    shipped
}

/// Release builds never read the local-server override from the environment.
pub(super) fn server_binary(shipped: PathBuf) -> PathBuf {
    #[cfg(debug_assertions)]
    let override_path = std::env::var_os("CINNABAR_LOCAL_SERVER");
    #[cfg(not(debug_assertions))]
    let override_path = None;
    select_binary(shipped, override_path, cfg!(debug_assertions))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_configuration_ignores_override() {
        let shipped = PathBuf::from("shipped");
        assert_eq!(
            select_binary(shipped.clone(), Some("custom".into()), false),
            shipped
        );
        let selected = select_binary(shipped, Some("custom".into()), true);
        assert!(selected.is_absolute());
        assert_eq!(selected, std::env::current_dir().unwrap().join("custom"));
    }

    /// A resolved startup catalog is shared and never refreshed after a rebuild.
    #[cfg(unix)]
    #[test]
    fn discovery_resolves_once_without_refreshing_the_binary() {
        use crate::local_worlds::test_support::write_script;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("server");
        write_script(&binary, b"#!/bin/sh\necho called >> \"$0.calls\"\necho '[{\"id\":\"islands\",\"label\":\"Islands\"}]'\n");
        let catalog = start_discovery(binary.clone());
        let shared = Arc::clone(&catalog);
        test_time::eventually("catalog discovery", || catalog.get().is_some());
        assert_eq!(
            catalog.get().unwrap().as_ref().unwrap()[0].id.as_str(),
            "islands"
        );
        write_script(&binary, b"#!/bin/sh\necho called >> \"$0.calls\"\necho '[{\"id\":\"void\",\"label\":\"Void\"}]'\n");
        assert_eq!(
            shared.get().unwrap().as_ref().unwrap()[0].id.as_str(),
            "islands"
        );
        assert_eq!(
            std::fs::read_to_string(binary.with_extension("calls")).unwrap(),
            "called\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn discovery_errors_are_resolved_once_too() {
        let catalog = start_discovery(PathBuf::from("/missing-local-server"));
        test_time::eventually("catalog failure", || catalog.get().is_some());
        assert!(catalog.get().unwrap().is_err());
    }
}
