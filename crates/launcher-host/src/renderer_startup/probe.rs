//! Isolated subprocess transport; driver calls never run in the launcher process.

use anyhow::{Context, Result};
use launcher::graphics_backend::Verdict;
#[cfg(any(not(target_os = "android"), test))]
use launcher::graphics_backend::probe_verdict;
use serde::{Deserialize, Serialize};
#[cfg(any(not(target_os = "android"), test))]
use std::time::Duration;

/// A transport result retains the exit verdict and bounded inventory output.
#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub verdict: Verdict,
    pub stdout: String,
}

#[cfg(target_os = "android")]
static RUNNER: std::sync::OnceLock<fn(&[String]) -> Result<Output>> = std::sync::OnceLock::new();

/// Installs Android's private-process service transport before starting the renderer.
#[cfg(target_os = "android")]
pub fn set_runner(runner: fn(&[String]) -> Result<Output>) -> Result<()> {
    RUNNER
        .set(runner)
        .map_err(|_| anyhow::anyhow!("GPU probe transport already installed"))
}

/// Android loads a library in an activity, so its isolated service replaces executable spawning.
#[cfg(target_os = "android")]
pub fn run(args: &[String]) -> Result<Output> {
    RUNNER
        .get()
        .context("Android GPU probe service unavailable")?(args)
}

/// Runs only our executable, with output in a file so a blocked stdout pipe cannot defeat the timeout.
#[cfg(not(target_os = "android"))]
pub fn run(args: &[String]) -> Result<Output> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        time::Instant,
    };
    let output = tempfile::tempfile()?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().context("start isolated GPU probe")?;
    let start = Instant::now();
    let verdict = supervise(
        &mut child,
        || start.elapsed(),
        || std::thread::sleep(Duration::from_millis(10)),
    )?;
    let mut stdout = String::new();
    // Clones share a file offset; reopen reading from the beginning after the child has stopped.
    use std::io::{Seek, SeekFrom};
    let mut output = output;
    output.seek(SeekFrom::Start(0))?;
    output.take(64 * 1024).read_to_string(&mut stdout)?;
    Ok(Output { verdict, stdout })
}

/// Runs and persists only a missing verdict; transport errors leave the driver unclassified.
pub fn cached(
    root: &std::path::Path,
    cache: &mut launcher::graphics_backend::ProbeCache,
    key: &launcher::graphics_backend::AdapterKey,
    backend: launcher::graphics_backend::Backend,
    execute: impl FnOnce() -> Result<Output>,
) -> Result<Verdict> {
    if let Some(verdict) = cache.get(key, backend) {
        return Ok(verdict);
    }
    let verdict = execute()?.verdict;
    cache.record(key.clone(), backend, verdict);
    super::save_probe_cache(root, cache)?;
    Ok(verdict)
}

/// The minimal process contract allows deterministic timeout tests without sleeping.
#[cfg(any(not(target_os = "android"), test))]
trait Process {
    /// Reports a completed exit without blocking on the process.
    fn exited(&mut self) -> Result<Option<bool>>;
    /// Stops only this operation and releases its process resources.
    fn stop(&mut self) -> Result<()>;
}

#[cfg(not(target_os = "android"))]
impl Process for std::process::Child {
    /// Reports whether this exact child exited successfully, including signal deaths.
    fn exited(&mut self) -> Result<Option<bool>> {
        Ok(self.try_wait()?.map(|exit| exit.success()))
    }
    /// Kills and reaps only the child created for this probe.
    fn stop(&mut self) -> Result<()> {
        let killed = self.kill();
        let waited = self.wait();
        waited.context("reap GPU probe")?;
        killed.context("stop GPU probe")
    }
}

/// Polls an isolated operation against a caller-owned clock and stops it on timeout or poll error.
#[cfg(any(not(target_os = "android"), test))]
fn supervise(
    process: &mut impl Process,
    mut elapsed: impl FnMut() -> Duration,
    mut wait: impl FnMut(),
) -> Result<Verdict> {
    loop {
        let exit = match process.exited() {
            Ok(exit) => exit,
            Err(error) => {
                process.stop()?;
                return Err(error);
            }
        };
        if let Some(verdict) = probe_verdict(exit, elapsed()) {
            if verdict == Verdict::TimedOut {
                process.stop()?;
            }
            return Ok(verdict);
        }
        wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::graphics_backend::PROBE_TIMEOUT;
    struct Child {
        exit: Option<bool>,
        stops: u32,
    }
    impl Process for Child {
        /// Returns the synthetic exit status without invoking an OS process.
        fn exited(&mut self) -> Result<Option<bool>> {
            Ok(self.exit)
        }
        /// Records exactly one cancellation of this synthetic child.
        fn stop(&mut self) -> Result<()> {
            self.stops += 1;
            Ok(())
        }
    }
    #[test]
    fn cached_verdicts_never_start_another_probe_and_driver_changes_do() {
        use launcher::graphics_backend::{AdapterKey, Backend, ProbeCache};
        let root = tempfile::tempdir().unwrap();
        let mut key = AdapterKey {
            name: "test".into(),
            vendor: 1,
            device: 2,
            driver: "test".into(),
            driver_info: String::new(),
            driver_version: "1.0".into(),
        };
        for verdict in [Verdict::Passed, Verdict::Failed, Verdict::TimedOut] {
            let mut cache = ProbeCache::default();
            assert_eq!(
                cached(root.path(), &mut cache, &key, Backend::Vulkan, || Ok(
                    Output {
                        verdict,
                        stdout: String::new()
                    }
                ))
                .unwrap(),
                verdict
            );
            let mut reloaded = super::super::load_probe_cache(root.path()).unwrap();
            assert_eq!(
                cached(
                    root.path(),
                    &mut reloaded,
                    &key,
                    Backend::Vulkan,
                    || panic!("cached driver must not be probed")
                )
                .unwrap(),
                verdict
            );
            key.driver_version.push_str(".1");
            assert_eq!(
                cached(root.path(), &mut reloaded, &key, Backend::Vulkan, || Ok(
                    Output {
                        verdict: Verdict::Passed,
                        stdout: String::new()
                    }
                ))
                .unwrap(),
                Verdict::Passed
            );
        }
    }

    #[test]
    fn transport_errors_do_not_mark_a_driver_bad() {
        use launcher::graphics_backend::{AdapterKey, Backend, ProbeCache};
        let root = tempfile::tempdir().unwrap();
        let key = AdapterKey {
            name: "test".into(),
            vendor: 1,
            device: 2,
            driver: "test".into(),
            driver_info: String::new(),
            driver_version: "1.0".into(),
        };
        let mut cache = ProbeCache::default();
        assert!(
            cached(
                root.path(),
                &mut cache,
                &key,
                Backend::Vulkan,
                || anyhow::bail!("executable unavailable")
            )
            .is_err()
        );
        assert_eq!(cache.get(&key, Backend::Vulkan), None);
    }

    #[test]
    fn timeout_stops_and_reaps_only_the_owned_child() {
        let mut child = Child {
            exit: None,
            stops: 0,
        };
        let mut elapsed = Duration::ZERO;
        let verdict = supervise(
            &mut child,
            || {
                elapsed += PROBE_TIMEOUT;
                elapsed
            },
            || panic!("deadline already reached"),
        )
        .unwrap();
        assert_eq!(verdict, Verdict::TimedOut);
        assert_eq!(child.stops, 1);
    }
    #[test]
    fn completed_crash_and_success_do_not_kill_a_finished_child() {
        for (exit, verdict) in [(false, Verdict::Failed), (true, Verdict::Passed)] {
            let mut child = Child {
                exit: Some(exit),
                stops: 0,
            };
            assert_eq!(
                supervise(&mut child, || Duration::ZERO, || panic!("already exited")).unwrap(),
                verdict
            );
            assert_eq!(child.stops, 0);
        }
    }
}
