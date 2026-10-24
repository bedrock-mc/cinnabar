//! Android first-run preparation of the shared dedicated-server install and startup code.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crossbeam_channel::RecvTimeoutError;
use launcher::{install_layout::InstallLayout, local_worlds::world::SetupState};

use super::{
    cancel_pair,
    provision::{Platform, Provisioner},
    server_pin,
    translated::Translation,
    world::{WorldError, failed},
};

/// How often the setup worker checks cancellation and forwards download progress.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Whether this Android install still needs its server download or startup-code preparation.
/// Other platforms and installs without the bundled runtime need no Android preparation.
pub fn android_server_needs_preparation(layout: &InstallLayout) -> bool {
    let Some((provisioner, translation)) = setup(layout) else {
        return false;
    };
    provisioner
        .installed()
        .is_none_or(|(binary, _)| !translation.is_prepared(&binary))
}

/// Downloads and prepares Android local worlds after the caller resolves Minecraft EULA
/// consent. The work finishes before the client starts, and cancellation saves/stops any
/// preparation process before returning. No saved user world is opened.
pub fn prepare_android_server(
    layout: &InstallLayout,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(&str),
) -> Result<(), WorldError> {
    let Some((provisioner, translation)) = setup(layout) else {
        return Ok(());
    };
    if cancelled.load(Ordering::Acquire) {
        return Err(WorldError::Cancelled);
    }
    provisioner.accept_eula()?;
    // Each call is a new setup attempt. A failed process is returned to the UI, never retried
    // here; a later Retry can repair code without touching the installed server or any world.
    if let Some((binary, _)) = provisioner.installed() {
        translation.retry_preparation(&binary)?;
    }
    run_setup(&provisioner, cancelled, &mut progress)
}

/// Registers setup preparation and its readiness check; normal world starts only ensure installation.
pub(super) fn configure_preparation(provisioner: &Provisioner, translation: &Arc<Translation>) {
    let preparing = Arc::clone(translation);
    let checking = Arc::clone(translation);
    provisioner.set_preparer(
        Arc::new(move |binary, provisioner, cancel| preparing.prepare(binary, provisioner, cancel)),
        Arc::new(move |binary| checking.is_prepared(binary)),
    );
}

/// Finds the APK runtime without opening the world store or creating a manager.
fn setup(layout: &InstallLayout) -> Option<(Arc<Provisioner>, Arc<Translation>)> {
    let translation = Arc::new(Translation::detect(
        Platform::host(),
        &layout.core_executable,
        super::android_runtime_dir(&layout.user_data_root),
    )?);
    let provisioner = Arc::new(Provisioner::new(
        layout.bds_dir(),
        server_pin().server_version.clone(),
    ));
    provisioner.set_runtime(translation.runtime());
    configure_preparation(&provisioner, &translation);
    Some((provisioner, translation))
}

/// Keeps progress and cancellation on the caller's thread while the serialized setup runs.
fn run_setup(
    provisioner: &Provisioner,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(&str),
) -> Result<(), WorldError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(WorldError::Cancelled);
    }
    let (handle, cancel) = cancel_pair();
    thread::scope(|scope| {
        let (result, received) = crossbeam_channel::bounded(1);
        let worker = scope.spawn(move || {
            let _ = result.send(provisioner.ensure_prepared(&cancel));
        });
        let mut handle = Some(handle);
        loop {
            if cancelled.load(Ordering::Acquire) {
                drop(handle.take());
            }
            let setup = provisioner.status().0;
            let line = match setup.state {
                SetupState::Downloading if setup.bytes_total > 0 => format!(
                    "Downloading local world server: {} / {} MB",
                    setup.bytes_done / 1_000_000,
                    setup.bytes_total / 1_000_000,
                ),
                SetupState::Downloading => "Downloading local world server".to_owned(),
                _ => "Preparing local worlds".to_owned(),
            };
            progress(&line);
            match received.recv_timeout(PROGRESS_INTERVAL) {
                Ok(result) => {
                    worker
                        .join()
                        .map_err(|_| failed("local world setup worker failed"))?;
                    if cancelled.load(Ordering::Acquire) {
                        return Err(WorldError::Cancelled);
                    }
                    return result.map(|_| ());
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(failed("local world setup worker stopped"));
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    })
}

#[cfg(test)]
#[path = "android_setup_tests.rs"]
mod tests;
