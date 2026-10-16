//! The game's side of updates: it holds the no-live-game gate for its whole process tree and
//! confirms a first start the update helper waits on. The desktop launcher checks and installs.

use anyhow::{Context, Result};
use launcher::{desktop::GameExit, install_layout::InstallLayout};
use std::{
    path::PathBuf,
    process::Command,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use update_installer::{runtime, startup};

static LAYOUT: OnceLock<InstallLayout> = OnceLock::new();
static SESSION: OnceLock<runtime::Session> = OnceLock::new();
/// Good frames confirmed this start, so a later failure is no longer a failed update.
static STARTED: AtomicBool = AtomicBool::new(false);
static RETURN_FOR_UPDATE: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
static STARTUP: OnceLock<update_installer::recovery::Startup> = OnceLock::new();

/// Whether a desktop launcher started this game and waits for its exit.
pub fn supervised() -> bool {
    std::env::var_os(launcher::desktop::SUPERVISED_ENV).is_some()
}

/// Records the player's Return to launcher choice for [`exit_code`].
pub fn return_to_launcher() {
    RETURN_FOR_UPDATE.store(true, Ordering::Release);
}

/// The exit status that tells the launcher why this game quit.
pub fn exit_code() -> i32 {
    if RETURN_FOR_UPDATE.load(Ordering::Acquire) {
        GameExit::ReturnForUpdate
    } else {
        GameExit::Quit
    }
    .code()
}

/// Joins the installation's running sessions, waiting out an install in progress, so no
/// update replaces files under this process or the helpers it shares the gate with.
pub fn join_installation(layout: &InstallLayout) -> Result<()> {
    let _ = LAYOUT.set(layout.clone());
    let joined_since = Instant::now();
    match runtime::Session::join(
        &layout.update_dir(),
        runtime::JOIN_TIMEOUT,
        || joined_since.elapsed(),
        || std::thread::sleep(update_installer::lease::POLL_INTERVAL),
    ) {
        Ok(session) => {
            let _ = SESSION.set(session);
        }
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
            return Err(error).context("wait for the Cinnabar update to finish");
        }
        // An unwritable update directory cannot host an install either; play continues.
        Err(error) => tracing::warn!("could not join the installation's update gate: {error}"),
    }
    #[cfg(unix)]
    if let Ok(startup) = installed_target().and_then(update_installer::recovery::Startup::observe) {
        let _ = STARTUP.set(startup);
    }
    Ok(())
}

/// Lets an installed helper `command` (core, crash monitor, setup) hold the gate while it runs.
pub fn share_installation(command: &mut Command) {
    if let Some(session) = SESSION.get() {
        session.share_with(command);
    }
}

/// Signals a complete first frame to the waiting helper and confirms any retained rollback.
pub fn acknowledge_startup() {
    let Some(layout) = LAYOUT.get() else {
        return;
    };
    STARTED.store(true, Ordering::Release);
    let marker = startup_marker(layout);
    #[cfg(unix)]
    let update_dir = layout.update_dir();
    std::thread::spawn(move || {
        if let Some(marker) = marker {
            let _ = std::fs::File::options()
                .write(true)
                .create_new(true)
                .open(marker);
        }
        #[cfg(unix)]
        if let Some(startup) = STARTUP.get()
            && let Err(error) = startup.confirm(&update_dir)
        {
            tracing::debug!("update recovery: {error:#}");
        }
    });
}

/// Lets a copied helper restore an orphaned replacement only after this failed client has exited.
pub fn recover_after_failed_start() {
    #[cfg(unix)]
    if !STARTED.load(Ordering::Acquire)
        && let Some(layout) = LAYOUT.get()
        && let Ok(target) = installed_target()
        && update_installer::recovery::needs_rollback(&target, &layout.update_dir())
        && let Err(error) = spawn_recovery(layout, target)
    {
        tracing::warn!("could not start update recovery: {error:#}");
    }
}

/// Runs `prepare`, relaying each change to its `progress` report to a waiting update helper;
/// `awaiting_user` marks reports during which the helper must not time out.
pub fn while_preparing<T>(
    layout: &InstallLayout,
    progress: &std::path::Path,
    awaiting_user: fn(&[u8]) -> bool,
    prepare: impl FnOnce() -> T,
) -> T {
    let Some(marker) = startup_marker(layout) else {
        return prepare();
    };
    let mut relay = startup::Relay::start(marker, progress.to_owned(), awaiting_user);
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let watcher = std::thread::spawn(move || {
        while stopped.recv_timeout(startup::PROGRESS_POLL)
            == Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        {
            relay.poll();
        }
        relay
    });
    let result = prepare();
    drop(stop);
    if let Ok(relay) = watcher.join() {
        relay.finish();
    }
    result
}

/// Whether this process, or the launcher that started it, was launched by the update helper.
pub fn launched_by_updater() -> bool {
    std::env::var_os(startup::STARTUP_ENV).is_some()
}

/// The helper's marker, accepted only inside this install's update directory.
fn startup_marker(layout: &InstallLayout) -> Option<PathBuf> {
    let marker = PathBuf::from(std::env::var_os(startup::STARTUP_ENV)?);
    (marker.starts_with(layout.update_dir())
        && marker
            .file_name()
            .is_some_and(|name| name == startup::STARTED))
    .then_some(marker)
}

/// Resolves only native app bundles, MSI layouts and mounted AppImage source files.
#[cfg(unix)]
fn installed_target() -> Result<PathBuf> {
    update_installer::target::resolve(
        std::env::consts::OS,
        &std::env::current_exe()?,
        std::env::var_os(startup::APPIMAGE)
            .map(PathBuf::from)
            .as_deref(),
    )
}

/// Starts rollback recovery with the same exact-parent exit barrier as an ordinary apply.
#[cfg(unix)]
fn spawn_recovery(layout: &InstallLayout, target: PathBuf) -> Result<()> {
    let helper = std::env::current_exe()?
        .parent()
        .context("missing installed directory")?
        .join(update_installer::helper_filename());
    update_installer::helper::start(
        &layout.update_dir(),
        &helper,
        &[
            update_installer::recovery::RECOVER_FLAG.into(),
            layout.update_dir().into_os_string(),
            target.into_os_string(),
            std::process::id().to_string().into(),
        ],
    )?;
    Ok(())
}
