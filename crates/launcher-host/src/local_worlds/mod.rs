//! Local single-player worlds: the saved-world store, the official dedicated-server download and
//! the local server processes. The core only learns the open world's loopback address.

mod android_trust;
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
mod translated;
mod unpack;
mod world;

pub use android_trust::refresh as refresh_android_server_trust;
pub use core_link::{CoreLink, CoreRoute};
pub use import::{WORLD_ARCHIVE_EXTENSION, is_world_archive};
pub use manager::{Host, Manager};
pub use world::WorldError;

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
};

use bridge::Backend;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use launcher::install_layout::InstallLayout;
use serde::Deserialize;

const BEDROCK_TARGET_JSON: &str = include_str!("../../../../assets/bedrock-target.json");

/// Resolves the local server beside the core, using the APK's executable helper on Android.
fn local_server_path(core: &Path, platform: provision::Platform) -> PathBuf {
    let name = match platform.os {
        "android" => translated::android_runtime().local_server_library.clone(),
        "windows" => "bedrock-local-server.exe".to_owned(),
        _ => "bedrock-local-server".to_owned(),
    };
    core.with_file_name(name)
}

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
/// a native server or bundled translator probe Docker in the background, assuming a container
/// runtime meanwhile.
pub fn open(layout: &InstallLayout) -> Result<Manager, WorldError> {
    let pin = server_pin();
    let store = Arc::new(store::Store::open(layout.local_worlds_dir())?);
    let platform = provision::Platform::host();
    let translation = translated::Translation::detect(
        platform,
        &layout.core_executable,
        android_runtime_dir(&layout.user_data_root),
    );
    let (runtime, pending) = if let Some(translation) = &translation {
        (translation.runtime(), false)
    } else if platform.runs_bds() {
        (provision::RuntimeInfo::native(), false)
    } else if platform.runs_containers() {
        (provision::RuntimeInfo::checking(), true)
    } else {
        (docker::detect_runtime(platform, docker::DOCKER, &[]), false)
    };
    store.set_default_backend(manager::default_backend(&runtime));
    let provisioner = Arc::new(provision::Provisioner::new(
        layout.bds_dir(),
        pin.server_version.clone(),
    ));
    provisioner.set_runtime(runtime.clone());
    let detected = translation.as_ref().map(translated::Translation::runtime);
    provisioner.set_detector(Arc::new(move || {
        detected
            .clone()
            .unwrap_or_else(|| docker::detect_runtime(platform, docker::DOCKER, &[]))
    }));
    let mut parts = manager::Parts::new(store);
    let local_server = local_server_path(&layout.core_executable, platform);
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
    let mut bds = bds::BdsRunner::new(Arc::clone(&provisioner), pin.bds_container_image.clone());
    bds.translation = translation;
    parts.runners.bds = Some(Arc::new(bds));
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

/// The Android server's exported platform certificate bundle, refreshed before each app launch.
pub fn android_server_trust_path(user_data_root: &Path) -> PathBuf {
    translated::trust_path(&android_runtime_dir(user_data_root))
}

/// Keeps the translated server's cache and trust store beneath one private runtime directory.
fn android_runtime_dir(user_data_root: &Path) -> PathBuf {
    user_data_root.join("bds-runtime")
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

    #[test]
    fn android_launches_the_packaged_server_from_the_native_library_directory() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../../packaging/android/runtime.json"))
                .expect("runtime manifest");
        let libraries = Path::new("/data/app/fixture/lib/arm64");
        let core = libraries.join(manifest["core_library"].as_str().expect("core helper"));
        let server = local_server_path(
            &core,
            provision::Platform {
                os: "android",
                arch: "arm64",
            },
        );
        assert_eq!(
            server,
            libraries.join(
                manifest["local_server_library"]
                    .as_str()
                    .expect("server helper")
            )
        );
        assert_eq!(server.extension().and_then(|ext| ext.to_str()), Some("so"));
    }

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
