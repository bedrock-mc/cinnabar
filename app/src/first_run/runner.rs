//! Runs the bundled `assetc prepare` and publishes the finished carriers.

use std::{
    ffi::OsString,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use assets::carriers;
use serde::Deserialize;

use super::fs_retry;

const CANCEL_POLL: Duration = Duration::from_millis(100);

/// The user stopped setup; the compiler is killed and nothing is published.
#[derive(Debug)]
pub(super) struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("setup was cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// What `assetc prepare --check` found stale.
#[derive(Debug, Deserialize)]
pub(super) struct Selection {
    pub stale: Vec<String>,
    pub needs_pack: bool,
}

impl Selection {
    pub(super) fn is_current(&self) -> bool {
        self.stale.is_empty()
    }
}

/// One progress line from `assetc prepare --json`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(super) enum Event {
    Plan {
        stale: Vec<String>,
    },
    Start {
        name: String,
        label: String,
    },
    Done {
        name: String,
    },
    Failed {
        name: String,
        label: String,
        required: bool,
        error: String,
    },
}

/// The bundled compiler, run against the kit's inputs with the pack below `workspace`.
pub(super) struct Compiler<'a> {
    pub kit: PathBuf,
    pub workspace: PathBuf,
    /// Receives both output streams; `None` discards them.
    pub log: Option<File>,
    /// Set to stop the running compiler and the run.
    pub cancel: &'a AtomicBool,
}

impl Compiler<'_> {
    fn prepare_args(&self, out: &Path) -> Vec<OsString> {
        vec![
            "prepare".into(),
            "--kit".into(),
            self.kit.clone().into(),
            "--workspace".into(),
            self.workspace.clone().into(),
            "--out".into(),
            out.into(),
        ]
    }

    /// The carriers in `out` that the kit's pins make stale.
    pub(super) fn check(&self, out: &Path) -> Result<Selection> {
        let mut args = self.prepare_args(out);
        args.push("--check".into());
        let mut selection = None;
        self.run(&args, |line| {
            if let Ok(found) = serde_json::from_str(line) {
                selection = Some(found);
            }
        })?;
        selection.context("the asset compiler reported no preparation plan")
    }

    /// Rebuilds the stale carriers in `out`, handing each progress event to `progress`.
    pub(super) fn prepare(&self, out: &Path, progress: impl FnMut(&Event)) -> Result<()> {
        let mut args = self.prepare_args(out);
        args.push("--json".into());
        self.prepare_with(&args, progress)
    }

    /// A required carrier's failure surfaces as its label and error, which the dialog shows.
    fn prepare_with(&self, args: &[OsString], mut progress: impl FnMut(&Event)) -> Result<()> {
        let mut required_failure = None;
        let result = self.run(args, |line| {
            let Ok(event) = serde_json::from_str::<Event>(line) else {
                return;
            };
            if let Event::Failed {
                label,
                required: true,
                error,
                ..
            } = &event
            {
                required_failure.get_or_insert_with(|| anyhow!("{label}: {error}"));
            }
            progress(&event);
        });
        match (result, required_failure) {
            (Err(error), _) if error.is::<Cancelled>() => Err(error),
            (Err(_), Some(failure)) => Err(failure),
            (result, _) => result,
        }
    }

    fn log_writer(&self) -> Result<Box<dyn Write + Send>> {
        Ok(match &self.log {
            Some(log) => Box::new(log.try_clone()?),
            None => Box::new(std::io::sink()),
        })
    }

    /// Runs the compiler, passing each stdout line to `line` and keeping both streams in the log;
    /// a failure carries the last stderr line.
    fn run(&self, args: &[OsString], mut line: impl FnMut(&str)) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let mut command = Command::new(carriers::kit_compiler(&self.kit));
        command.args(args);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command
            .current_dir(&self.workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        fs::create_dir_all(&self.workspace)?;
        let mut child = command.spawn().context("start the asset compiler")?;
        let stdout = child
            .stdout
            .take()
            .context("capture asset compiler output")?;
        let stderr = child
            .stderr
            .take()
            .context("capture asset compiler errors")?;
        let (lines, received) = mpsc::channel();
        let out_log = self.log_writer()?;
        let out = std::thread::spawn(move || {
            copy_lines(stdout, out_log, |text| {
                let _ = lines.send(text.to_owned());
            })
        });
        let err_log = self.log_writer()?;
        let tail = std::thread::spawn(move || copy_lines(stderr, err_log, |_| {}));
        let status = loop {
            while let Ok(text) = received.try_recv() {
                line(&text);
            }
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if self.cancel.load(Ordering::Relaxed) {
                cancel_child(&mut child);
                return Err(Cancelled.into());
            }
            if let Ok(text) = received.recv_timeout(CANCEL_POLL) {
                line(&text);
            }
        };
        let _ = out.join();
        for text in received.try_iter() {
            line(&text);
        }
        let last_line = tail.join().ok().flatten();
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        if status.success() {
            return Ok(());
        }
        Err(match last_line {
            Some(line) => anyhow!("{line} ({status})"),
            None => anyhow!("the asset compiler exited with {status}"),
        })
    }
}

/// Copies `output` into `log` line by line, hands each line to `each`, and returns the last
/// non-blank one.
fn copy_lines(
    output: impl Read,
    mut log: impl Write,
    mut each: impl FnMut(&str),
) -> Option<String> {
    let mut last = None;
    for line in BufReader::new(output).split(b'\n') {
        let Ok(line) = line else {
            break;
        };
        let _ = log.write_all(&line);
        let _ = log.write_all(b"\n");
        let text = String::from_utf8_lossy(&line).trim().to_owned();
        each(&text);
        if !text.is_empty() {
            last = Some(text);
        }
    }
    last
}

/// Copies the published carriers into `staged` so carriers that are still current carry over.
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
        fs_retry::remove_dir_all(&old).with_context(|| format!("remove {}", old.display()))?;
    }
    if final_dir.exists() {
        fs_retry::rename(final_dir, &old)
            .with_context(|| format!("move {} aside", final_dir.display()))?;
    }
    if let Err(error) = fs_retry::rename(staged, final_dir) {
        let _ = fs_retry::rename(&old, final_dir);
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
        let _ = fs_retry::rename(&old, final_dir);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::first_run::test_support::Dir;

    /// A compiler whose kit binary is `/bin/sh`, so tests drive it with `-c <script>`.
    #[cfg(unix)]
    fn shell_compiler<'a>(dir: &Path, cancel: &'a AtomicBool) -> Compiler<'a> {
        let compiler = carriers::kit_compiler(dir);
        fs::create_dir_all(compiler.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink("/bin/sh", compiler).unwrap();
        Compiler {
            kit: dir.into(),
            workspace: dir.into(),
            log: Some(File::create(dir.join("log")).unwrap()),
            cancel,
        }
    }

    #[cfg(unix)]
    fn script(text: &str) -> Vec<OsString> {
        vec!["-c".into(), text.into()]
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
    fn review_cancellation_stops_compiler_descendants() {
        let dir = Dir::new("cancel-descendants");
        let cancel = AtomicBool::new(false);
        let compiler = shell_compiler(dir.path(), &cancel);
        let args = script("sleep 30 & echo $! > descendant; wait");
        let pid = std::thread::scope(|scope| {
            let running = scope.spawn(|| compiler.run(&args, |_| {}));
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

    #[cfg(unix)]
    #[test]
    fn a_failing_compiler_reports_its_last_stderr_line() {
        let dir = Dir::new("compiler-stderr");
        let cancel = AtomicBool::new(false);
        let compiler = shell_compiler(dir.path(), &cancel);
        let args = script(
            "echo progress; echo 'warning: slow' >&2; echo 'missing texture atlas' >&2; echo >&2; exit 3",
        );
        let message = format!("{:#}", compiler.run(&args, |_| {}).unwrap_err());
        assert!(message.starts_with("missing texture atlas ("), "{message}");
        let log = fs::read_to_string(dir.path().join("log")).unwrap();
        assert!(
            log.contains("progress") && log.contains("warning: slow"),
            "{log}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_required_carrier_failure_surfaces_its_label_and_error() {
        let dir = Dir::new("compiler-required");
        let cancel = AtomicBool::new(false);
        let compiler = shell_compiler(dir.path(), &cancel);
        let failed = r#"{"event":"failed","name":"hud","label":"Compiling HUD sprites","required":true,"error":"missing atlas"}"#;
        let optional = r#"{"event":"failed","name":"weather","label":"Compiling weather textures","required":false,"error":"no rain"}"#;
        let args = script(&format!(
            "echo '{optional}'; echo '{failed}'; echo boom >&2; exit 1"
        ));
        let mut seen = Vec::new();
        let error = compiler
            .prepare_with(&args, |event| seen.push(event.clone()))
            .unwrap_err();
        assert_eq!(format!("{error:#}"), "Compiling HUD sprites: missing atlas");
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn progress_lines_decode_into_events() {
        let start: Event = serde_json::from_str(
            r#"{"event":"start","name":"world","label":"Compiling world assets"}"#,
        )
        .unwrap();
        assert_eq!(
            start,
            Event::Start {
                name: "world".into(),
                label: "Compiling world assets".into()
            }
        );
        let check: Selection =
            serde_json::from_str(r#"{"current":false,"stale":["font"],"needs_pack":false}"#)
                .unwrap();
        assert!(!check.is_current() && !check.needs_pack);
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

    #[test]
    fn a_failed_publish_restores_the_earlier_directory() {
        let dir = Dir::new("publish-rollback");
        let staged = dir.path().join("missing-staged");
        let final_dir = dir.path().join("compiled");
        fs::create_dir(&final_dir).unwrap();
        fs::write(final_dir.join("carrier"), b"old assets").unwrap();
        assert!(publish(&staged, &final_dir).is_err());
        assert_eq!(fs::read(final_dir.join("carrier")).unwrap(), b"old assets");
        assert!(!previous(&final_dir).exists());
    }
}
