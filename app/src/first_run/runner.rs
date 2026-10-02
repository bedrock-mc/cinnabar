//! Executes the preparation plan as child processes and publishes the finished carriers.

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::{Context, Result, bail};

use super::plan::{Action, Step};

const CANCEL_POLL: Duration = Duration::from_millis(100);

/// The user stopped setup; running steps are killed and nothing is published.
#[derive(Debug)]
pub(super) struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("setup was cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// Runs `steps` in order; a failed required step aborts, a failed optional step is returned as skipped.
pub(super) fn execute_steps(
    steps: &[Step],
    mut exec: impl FnMut(&Step) -> Result<()>,
    mut progress: impl FnMut(usize, &Step),
) -> Result<Vec<&'static str>> {
    let mut skipped = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        progress(index, step);
        if let Err(error) = exec(step) {
            if error.is::<Cancelled>() {
                return Err(error);
            }
            if step.required {
                return Err(error.context(step.label));
            }
            skipped.push(step.label);
        }
    }
    Ok(skipped)
}

/// Kit directories and the repo-relative workspace paths they stage to.
const KIT_LAYOUT: [(&str, &str); 3] = [
    ("scripts", "scripts"),
    ("assets", "assets"),
    ("data", "crates/assets/data"),
];

/// Copies the bundled scripts, manifests and registries into the workspace at repo-relative paths.
pub(super) fn stage_kit(kit: &Path, workspace: &Path) -> Result<()> {
    for (from, to) in KIT_LAYOUT {
        copy_tree(&kit.join(from), &workspace.join(to))?;
    }
    Ok(())
}

/// The kit file a workspace-relative path stages from, if the kit ships one.
pub(super) fn kit_file(kit: &Path, relative: &str) -> Option<PathBuf> {
    KIT_LAYOUT.iter().find_map(|(from, to)| {
        let rest = relative.strip_prefix(to)?.strip_prefix('/')?;
        let path = kit.join(from).join(rest);
        path.is_file().then_some(path)
    })
}

/// Copies the published carriers into `staged` so steps that are still current keep them.
pub(super) fn seed(prepared: &Path, staged: &Path) -> Result<()> {
    if prepared.is_dir() {
        copy_tree(prepared, staged)?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).with_context(|| format!("create {}", to.display()))?;
    for entry in fs::read_dir(from).with_context(|| format!("read {}", from.display()))? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)
                .with_context(|| format!("copy {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn previous(final_dir: &Path) -> PathBuf {
    final_dir.with_extension("previous")
}

/// Swaps the fully built carrier directory into place. The earlier set is only moved aside, so an
/// interruption leaves either set recoverable by [`recover`].
pub(super) fn publish(staged: &Path, final_dir: &Path) -> Result<()> {
    if let Some(parent) = final_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    let old = previous(final_dir);
    if old.exists() {
        fs::remove_dir_all(&old).with_context(|| format!("remove {}", old.display()))?;
    }
    if final_dir.exists() {
        fs::rename(final_dir, &old)
            .with_context(|| format!("move {} aside", final_dir.display()))?;
    }
    if let Err(error) = fs::rename(staged, final_dir) {
        let _ = fs::rename(&old, final_dir);
        return Err(error)
            .with_context(|| format!("move {} to {}", staged.display(), final_dir.display()));
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

/// Publishes completed preparation only while the run still has cancellation authority.
pub(super) fn publish_if_active(
    staged: &Path,
    final_dir: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled.into());
    }
    publish(staged, final_dir)
}

/// Restores the earlier carrier set when a publish was interrupted between its two renames.
pub(super) fn recover(final_dir: &Path) {
    let old = previous(final_dir);
    if !final_dir.exists() && old.is_dir() {
        let _ = fs::rename(&old, final_dir);
    }
}

/// Deletes a step's earlier outputs so a failed optional rerun cannot leave a stale carrier.
pub(super) fn clear_output(staged: &Path, name: &str) {
    let path = staged.join(name);
    let _ = if path.is_dir() {
        fs::remove_dir_all(&path)
    } else {
        fs::remove_file(&path)
    };
}

pub(super) struct ProcessExec<'a> {
    pub workspace: PathBuf,
    pub kit: PathBuf,
    pub log: File,
    /// Set to kill the running step and stop.
    pub cancel: &'a AtomicBool,
}

impl ProcessExec<'_> {
    pub(super) fn run(&self, step: &Step) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let mut command = self.command(&step.action)?;
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command
            .current_dir(&self.workspace)
            .stdin(Stdio::null())
            .stdout(self.log.try_clone()?)
            .stderr(self.log.try_clone()?);
        let mut child = command
            .spawn()
            .with_context(|| format!("start {}", step.label))?;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if self.cancel.load(Ordering::Relaxed) {
                cancel_child(&mut child);
                return Err(Cancelled.into());
            }
            std::thread::sleep(CANCEL_POLL);
        };
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        if !status.success() {
            bail!("{} exited with {status}; see the first-run log", step.label);
        }
        Ok(())
    }

    fn command(&self, action: &Action) -> Result<Command> {
        match action {
            Action::Assetc(args) => {
                let mut command = Command::new(self.kit.join("bin").join(assetc_name()));
                command.args(args);
                Ok(command)
            }
            Action::Script(name) => Ok(script_command(&self.kit, name)),
        }
    }
}

/// The bundled compiler executable name for this platform.
pub(super) const fn assetc_name() -> &'static str {
    if cfg!(windows) {
        "assetc.exe"
    } else {
        "assetc"
    }
}

/// Stops the isolated setup process tree and reaps its immediate child.
fn cancel_child(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = i32::try_from(child.id())
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

fn script_command(kit: &Path, name: &str) -> Command {
    if cfg!(windows) {
        let mut command = Command::new("powershell");
        command
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(format!("scripts\\{name}.ps1"));
        if name == "fetch-vanilla-assets" {
            command.arg("-AcceptEula");
        }
        command
    } else {
        let mut command = Command::new("bash");
        command.arg(format!("scripts/{name}.sh"));
        if name == "fetch-vanilla-assets" {
            command.arg("--accept-eula");
            let helper = kit.join("bin/rename-directory-no-replace");
            if helper.is_file() {
                command.env("CINNABAR_PUBLISHER_BINARY", helper);
            }
        }
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::first_run::test_support::Dir;

    fn step(label: &'static str, required: bool) -> Step {
        Step {
            label,
            action: Action::Script("x"),
            required,
        }
    }

    #[test]
    fn review_cancellation_after_the_last_step_prevents_publication() {
        let dir = Dir::new("late-cancel");
        let staged = dir.path().join("staged");
        let prepared = dir.path().join("prepared");
        fs::create_dir(&staged).unwrap();
        fs::write(staged.join("carrier"), b"new").unwrap();
        let result = publish_if_active(&staged, &prepared, &AtomicBool::new(true));
        assert!(result.is_err());
        assert!(!prepared.exists());
        assert!(staged.join("carrier").exists());
    }

    #[cfg(unix)]
    #[test]
    fn review_cancellation_stops_step_descendants() {
        let dir = Dir::new("cancel-descendants");
        fs::create_dir(dir.path().join("scripts")).unwrap();
        fs::write(
            dir.path().join("scripts/cancel-test.sh"),
            b"sleep 30 & echo $! > descendant; wait
",
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        let exec = ProcessExec {
            workspace: dir.path().into(),
            kit: dir.path().into(),
            log: File::create(dir.path().join("log")).unwrap(),
            cancel: &cancel,
        };
        let pid = std::thread::scope(|scope| {
            let running = scope.spawn(|| {
                exec.run(&Step {
                    label: "cancel",
                    required: true,
                    action: Action::Script("cancel-test"),
                })
            });
            let path = dir.path().join("descendant");
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !path.exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let raw: i32 = fs::read_to_string(path).unwrap().trim().parse().unwrap();
            cancel.store(true, Ordering::Relaxed);
            assert!(running.join().unwrap().unwrap_err().is::<Cancelled>());
            rustix::process::Pid::from_raw(raw).unwrap()
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while rustix::process::test_kill_process(pid).is_ok()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let alive = rustix::process::test_kill_process(pid).is_ok();
        if alive {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
        }
        assert!(!alive, "setup left its descendant running");
    }

    #[test]
    fn cancelling_an_optional_step_stops_the_run() {
        let steps = [step("a", false), step("b", true)];
        let mut ran = Vec::new();
        let error = execute_steps(
            &steps,
            |s| {
                ran.push(s.label);
                Err(Cancelled.into())
            },
            |_, _| {},
        )
        .unwrap_err();
        assert!(error.is::<Cancelled>());
        assert_eq!(ran, ["a"]);
    }

    #[test]
    fn required_failure_aborts_and_optional_failure_is_skipped() {
        let steps = [step("a", true), step("b", false), step("c", true)];
        let skipped = execute_steps(
            &steps,
            |s| if s.label == "b" { bail!("no") } else { Ok(()) },
            |_, _| {},
        )
        .unwrap();
        assert_eq!(skipped, ["b"]);

        let mut ran = Vec::new();
        let error = execute_steps(
            &steps,
            |s| {
                ran.push(s.label);
                if s.label == "a" {
                    bail!("boom")
                } else {
                    Ok(())
                }
            },
            |_, _| {},
        )
        .unwrap_err();
        assert_eq!(ran, ["a"]);
        assert!(format!("{error:#}").contains("boom"));
    }

    #[test]
    fn progress_reports_each_step_index_in_order() {
        let steps = [step("a", true), step("b", true)];
        let mut seen = Vec::new();
        execute_steps(&steps, |_| Ok(()), |index, s| seen.push((index, s.label))).unwrap();
        assert_eq!(seen, [(0, "a"), (1, "b")]);
    }

    #[test]
    fn stage_kit_maps_data_under_the_registry_path() {
        let dir = Dir::new("kit");
        let kit = dir.path().join("kit");
        for sub in ["scripts", "assets", "data"] {
            fs::create_dir_all(kit.join(sub)).unwrap();
            fs::write(kit.join(sub).join("f"), sub).unwrap();
        }
        let workspace = dir.path().join("ws");
        stage_kit(&kit, &workspace).unwrap();
        assert_eq!(
            fs::read(workspace.join("crates/assets/data/f")).unwrap(),
            b"data"
        );
        assert!(workspace.join("scripts/f").is_file());
        assert!(workspace.join("assets/f").is_file());
    }

    #[test]
    fn an_interrupted_publish_recovers_the_earlier_set() {
        let dir = Dir::new("recover");
        let final_dir = dir.path().join("compiled");
        // Crash after the old set moved aside but before the new one landed.
        fs::create_dir_all(previous(&final_dir)).unwrap();
        fs::write(previous(&final_dir).join("old"), b"1").unwrap();
        recover(&final_dir);
        assert!(final_dir.join("old").is_file() && !previous(&final_dir).exists());
        // With the final set present, a leftover aside copy is ignored.
        fs::create_dir_all(previous(&final_dir)).unwrap();
        recover(&final_dir);
        assert!(final_dir.join("old").is_file());
    }

    #[test]
    fn kit_files_resolve_through_the_staging_layout() {
        let dir = Dir::new("kit-file");
        fs::create_dir_all(dir.path().join("data")).unwrap();
        fs::write(dir.path().join("data/reg.bin"), b"r").unwrap();
        assert_eq!(
            kit_file(dir.path(), "crates/assets/data/reg.bin"),
            Some(dir.path().join("data/reg.bin"))
        );
        assert_eq!(kit_file(dir.path(), "crates/assets/data/missing.bin"), None);
        assert_eq!(kit_file(dir.path(), ".local/assets/compiled/x"), None);
    }

    #[test]
    fn publish_replaces_an_earlier_directory() {
        let dir = Dir::new("publish");
        let (staged, final_dir) = (dir.path().join("staged"), dir.path().join("out/final"));
        fs::create_dir_all(&staged).unwrap();
        fs::write(staged.join("new"), b"1").unwrap();
        fs::create_dir_all(&final_dir).unwrap();
        fs::write(final_dir.join("old"), b"1").unwrap();
        publish(&staged, &final_dir).unwrap();
        assert!(final_dir.join("new").is_file() && !final_dir.join("old").exists());
        assert!(!previous(&final_dir).exists());
    }
}
