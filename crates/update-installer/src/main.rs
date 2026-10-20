//! Offline apply process copied outside the installed app before the old client exits.
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use update_installer::startup::{self, Monitor, Progress};

/// Accepts only local inputs and reports a failed apply in the installer's log.
#[expect(
    clippy::print_stderr,
    reason = "the launcher captures this process's stderr into apply.log"
)]
fn main() {
    if let Err(error) = run() {
        eprintln!("Update was not installed: {error:#}");
        std::process::exit(1);
    }
}

/// Waits for the exact parent process before verifying, applying, and launching the release.
fn run() -> Result<()> {
    let _copy = update_installer::helper::hold(&std::env::current_exe()?)?;
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args
        .first()
        .is_some_and(|arg| arg == update_installer::recovery::RECOVER_FLAG)
    {
        ensure!(
            args.len() == 4,
            "expected --recover UPDATE_DIR TARGET PARENT_PID"
        );
        let (root, target) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
        let pid = args[3].to_str().context("invalid parent PID")?.parse()?;
        update_installer::parent::wait(pid)?;
        if update_installer::recovery::restore(&target, &root)? {
            launch(&target, &root)?;
        }
        return Ok(());
    }
    ensure!(args.len() == 4, "expected STAGE TARGET PARENT_PID CHANNEL");
    let (stage, target) = (PathBuf::from(&args[0]), PathBuf::from(&args[1]));
    let pid = args[2].to_str().context("invalid parent PID")?.parse()?;
    let channel =
        update_manifest::channel::Channel::parse(args[3].to_str().context("invalid channel")?)
            .context("unknown channel")?;
    let platform = update_manifest::platform_key(std::env::consts::OS, std::env::consts::ARCH)
        .context("unsupported platform")?;
    let keys = update_manifest::parse_keys(update_installer::TRUSTED_KEYS)?;
    let snapshot_reader = update_installer::helper::hold_snapshot(&stage);
    update_installer::parent::wait(pid)?;
    let root = stage.parent().context("missing update directory")?;
    let mut started = false;
    let result = (|| {
        let _snapshot_reader = snapshot_reader.map_err(|error| {
            anyhow::Error::new(error).context(update_installer::apply::Untouched)
        })?;
        let waiting_since = Instant::now();
        let lease = update_installer::lease::Lease::wait(
            root,
            update_installer::lease::WAIT_TIMEOUT,
            || waiting_since.elapsed(),
            || std::thread::sleep(update_installer::lease::POLL_INTERVAL),
        )
        .map_err(|error| anyhow::Error::new(error).context(update_installer::apply::Untouched))?;
        let installed = update_installer::target::installed_build(&target)
            .context(update_installer::apply::Untouched)?;
        update_installer::apply::after_exit(
            update_installer::apply::Request {
                stage_dir: &stage,
                target: &target,
                keys: &keys,
                check: update_manifest::select::Check {
                    channel: channel.name(),
                    platform: &platform,
                    current: &installed.version,
                    current_build: installed.number,
                    switch_now: false,
                },
                now: time::OffsetDateTime::now_utc(),
            },
            lease,
            |target| {
                let result = launch(target, root);
                started = result.is_ok();
                result
            },
        )
    })();
    finish_apply(result, started, || relaunch_retained(&target))
}

/// Restarts after a failed apply, except when the replacement already confirmed good startup.
fn finish_apply(
    result: Result<()>,
    started: bool,
    restart: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if result.is_err() && !started {
        // Any required rollback ran under the apply lease; an untouched or busy stage needs none.
        restart()?;
    }
    result
}

/// Confirms the new client reached a complete frame, restoring its predecessor on early failure.
/// Asset preparation heartbeats restart the stall timeout, so a long first-run download is not a failure.
fn launch(target: &Path, root: &Path) -> Result<()> {
    let directory = tempfile::Builder::new().prefix("start-").tempdir_in(root)?;
    let mut command = startup::client_command(target);
    command.env(startup::STARTUP_ENV, startup::marker(directory.path()));
    let mut client = startup::Tree::spawn(&mut command).context("launch updated client")?;
    let mut monitor = Monitor::new(Instant::now());
    loop {
        let failure = match monitor.poll(directory.path(), Instant::now()) {
            Progress::Started => return Ok(()),
            Progress::Stalled => Some("updated client did not complete startup".to_owned()),
            Progress::Waiting => client
                .try_wait()?
                .map(|status| format!("updated client exited before startup: {status}")),
        };
        if let Some(failure) = failure {
            // Rollback restores files only after the client and its setup process have exited.
            client.stop()?;
            anyhow::bail!(failure);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Restarts a retained client without requiring a writable update directory or a rollback lease.
fn relaunch_retained(target: &Path) -> Result<()> {
    startup::Tree::spawn(&mut startup::client_command(target))
        .context("restart the retained installation")
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_relaunch_does_not_reuse_another_attempts_startup_receipt() {
        let command = startup::client_command(Path::new("retained-client"));
        assert!(
            command
                .get_envs()
                .any(|(name, value)| name == startup::STARTUP_ENV && value.is_none())
        );
    }

    #[test]
    fn a_busy_stage_at_restart_still_relaunches_the_untouched_client() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("retained-client");
        std::fs::write(&target, "retained installation").unwrap();
        let _other_client = update_installer::lease::Lease::acquire(root.path()).unwrap();
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let result = update_installer::lease::Lease::wait(
            root.path(),
            update_installer::lease::WAIT_TIMEOUT,
            || elapsed.get(),
            || elapsed.set(update_installer::lease::WAIT_TIMEOUT),
        )
        .map(|_| ())
        .map_err(|error| anyhow::Error::new(error).context(update_installer::apply::Untouched));
        assert!(
            result
                .as_ref()
                .unwrap_err()
                .is::<update_installer::apply::Untouched>()
        );
        let mut relaunched = false;
        let error = finish_apply(result, false, || {
            // Relaunch needs no access to the lease still owned by the downloading client.
            assert!(update_installer::lease::Lease::try_acquire(root.path()).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"retained installation");
            relaunched = true;
            Ok(())
        })
        .unwrap_err();
        assert!(relaunched);
        assert!(error.is::<update_installer::apply::Untouched>());
    }

    #[test]
    fn a_refused_update_restarts_the_retained_client_and_keeps_the_original_error() {
        let mut restarted = false;
        let error = finish_apply(
            Err(anyhow::anyhow!("manifest expired").context(update_installer::apply::Untouched)),
            false,
            || {
                restarted = true;
                Ok(())
            },
        )
        .unwrap_err();
        assert!(restarted);
        assert!(format!("{error:#}").contains("manifest expired"));
    }

    #[test]
    fn cleanup_failure_after_good_start_does_not_launch_a_second_client() {
        assert!(
            finish_apply(Err(anyhow::anyhow!("cleanup failed")), true, || panic!(
                "already running replacement must not be launched twice"
            ))
            .is_err()
        );
        finish_apply(Ok(()), true, || {
            panic!("already running replacement must not be launched twice")
        })
        .unwrap();
    }
}
