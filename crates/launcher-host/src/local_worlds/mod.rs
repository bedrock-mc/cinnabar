//! Local single-player worlds: the saved-world store, the official dedicated-server download and
//! the local server processes. The core only learns the open world's loopback address.

mod bds;
mod container;
mod core_link;
mod docker;
mod download;
mod import;
mod junction;
mod level_dat;
mod manager;
mod ports;
mod process;
mod provision;
mod store;
#[cfg(test)]
mod test_support;
mod unpack;
mod world;

pub use core_link::{CoreLink, CoreRoute};
pub use import::{WORLD_ARCHIVE_EXTENSION, is_world_archive};
pub use manager::Manager;
pub use world::WorldError;

use std::{
    fs, io,
    path::Path,
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
};

use bridge::Backend;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use launcher::install_layout::InstallLayout;
use serde::Deserialize;

const BEDROCK_TARGET_JSON: &str = include_str!("../../../../assets/bedrock-target.json");
/// The Dragonfly local server, shipped beside the core.
const LOCAL_SERVER: &str = if cfg!(windows) {
    "bedrock-local-server.exe"
} else {
    "bedrock-local-server"
};

/// The dedicated-server build local worlds run, from the target manifest.
#[derive(Debug, Deserialize)]
struct ServerPin {
    server_version: String,
    bds_container_image: String,
}

fn server_pin() -> &'static ServerPin {
    static PIN: OnceLock<ServerPin> = OnceLock::new();
    PIN.get_or_init(|| {
        serde_json::from_str(BEDROCK_TARGET_JSON).expect("valid Bedrock target manifest")
    })
}

/// Opens the install's local worlds on the manifest's exact server build and image. Hosts without
/// a native server probe for Docker in the background, assuming the container runtime meanwhile.
pub fn open(layout: &InstallLayout) -> Result<Manager, WorldError> {
    let pin = server_pin();
    let store = Arc::new(store::Store::open(layout.local_worlds_dir())?);
    let platform = provision::Platform::host();
    let (runtime, pending) = if platform.runs_bds() {
        (provision::RuntimeInfo::native(), false)
    } else {
        (provision::RuntimeInfo::checking(), true)
    };
    store.set_default_backend(manager::default_backend(&runtime));
    let provisioner = Arc::new(provision::Provisioner::new(
        layout.bds_dir(),
        pin.server_version.clone(),
    ));
    provisioner.set_runtime(runtime.clone());
    provisioner.set_detector(Arc::new(move || {
        docker::detect_runtime(platform, docker::DOCKER, &[])
    }));
    let mut parts = manager::Parts::new(store);
    let local_server = layout.core_executable.with_file_name(LOCAL_SERVER);
    if local_server.is_file() {
        parts.runners.dragonfly = Some(Arc::new(process::ProcessRunner::new(local_server)));
    } else {
        // Docker detection may still settle on Dragonfly; its worlds are then refused, not saved unopenable.
        parts.unavailable.push((
            Backend::Dragonfly,
            format!(
                "local world server binary not found at {}; build it with `make local-server`",
                local_server.display()
            ),
        ));
    }
    parts.runners.bds = Some(Arc::new(bds::BdsRunner::new(
        Arc::clone(&provisioner),
        pin.bds_container_image.clone(),
    )));
    parts.setup = Some(Arc::clone(&provisioner));
    parts.auto_backend = true;
    let manager = Manager::from_parts(parts);
    tracing::info!(
        "local worlds in {} ({}: {})",
        layout.local_worlds_dir().display(),
        runtime.kind.as_str(),
        runtime.reason
    );
    if pending {
        provisioner.detect_in_background(runtime);
    }
    Ok(manager)
}

/// A manager over the worlds in `root` with no server runners, for code that drives one in tests.
#[cfg(any(test, feature = "test-support"))]
pub fn open_store(root: std::path::PathBuf) -> Result<Manager, WorldError> {
    let store = Arc::new(store::Store::open(root)?);
    Ok(Manager::from_parts(manager::Parts::new(store)))
}

/// Ends a start or another wait once its [`CancelHandle`] is dropped.
#[derive(Clone)]
pub(crate) struct Cancel(Receiver<()>);

/// Cancels its [`Cancel`] when dropped.
pub(crate) struct CancelHandle {
    _sender: Sender<()>,
}

pub(crate) fn cancel_pair() -> (CancelHandle, Cancel) {
    let (sender, receiver) = crossbeam_channel::bounded(0);
    (CancelHandle { _sender: sender }, Cancel(receiver))
}

impl Cancel {
    /// A cancellation that never fires.
    pub(crate) fn never() -> Self {
        Self(crossbeam_channel::never())
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        matches!(self.0.try_recv(), Err(TryRecvError::Disconnected))
    }

    /// Disconnects once cancelled, for `select!`.
    pub(crate) fn receiver(&self) -> &Receiver<()> {
        &self.0
    }
}

/// Locks `mutex`, recovering the data if a holder panicked.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Creates `path` and its parents, readable only by this user on Unix.
pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

/// Writes `bytes` to `path`, creating it readable only by this user on Unix.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    io::Write::write_all(&mut options.open(path)?, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tag alone drifts with upstream releases and can break joins; the image must name a digest.
    #[test]
    fn the_container_image_is_pinned_to_a_digest() {
        let image = &server_pin().bds_container_image;
        let (name, digest) = image.split_once("@sha256:").expect("digest-pinned image");
        assert!(!name.is_empty() && !name.ends_with(":latest"), "{image}");
        assert!(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "{image}"
        );
        let version = &server_pin().server_version;
        assert_eq!(version.split('.').count(), 4, "exact build: {version}");
    }

    #[test]
    fn dropping_the_handle_cancels_every_clone() {
        let (handle, cancel) = cancel_pair();
        let clone = cancel.clone();
        assert!(!cancel.is_cancelled());
        drop(handle);
        assert!(cancel.is_cancelled() && clone.is_cancelled());
        assert!(!Cancel::never().is_cancelled());
    }
}
