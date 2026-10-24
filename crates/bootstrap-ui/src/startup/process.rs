//! Responsive startup feedback in a process independent of the game's event loop.

use anyhow::{Context, Result};
use launcher::startup::{Event, Message, Stage};
use launcher_host::lifecycle::children::{self, EXIT_GRACE, Spawned};
use std::{
    io::{BufRead, BufReader, Write},
    process::{ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

/// Selects the loading-window process before ordinary client argument parsing.
use super::FLAG;
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// Owns the loading process and closes it on every return or unwind from client startup.
pub struct Process {
    child: Arc<Spawned>,
    input: Arc<Mutex<Option<ChildStdin>>>,
    cancelled: Arc<AtomicBool>,
    lifetime: mpsc::Sender<Notice>,
}

/// Wakes the process owner when the first frame arrives or feedback is no longer needed.
enum Notice {
    Shown,
    Retire,
}

impl Process {
    /// Spawns feedback without waiting for its window or graphics device to initialize.
    pub fn start(size: [f32; 2]) -> Result<Self> {
        let child = children::spawn(
            Command::new(std::env::current_exe()?)
                .arg(FLAG)
                .args(size.map(|value| value.to_string()))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit()),
        )
        .context("open startup screen")?;
        Self::from_child(child, FIRST_FRAME_TIMEOUT)
    }

    /// Connects the feedback protocol to an already owned helper process.
    fn from_child(child: Spawned, first_frame_timeout: Duration) -> Result<Self> {
        let input = Arc::new(Mutex::new(child.take_stdin()));
        let output = child.take_stdout().context("startup screen output")?;
        let child = Arc::new(child);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let (lifetime, events) = mpsc::channel();
        let notices = lifetime.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                match serde_json::from_str::<Event>(&line) {
                    Ok(Event::Shown) => {
                        let _ = notices.send(Notice::Shown);
                    }
                    Ok(Event::Cancelled) => {
                        flag.store(true, Ordering::Release);
                    }
                    Err(_) => {}
                }
            }
        });
        let process = child.clone();
        let transport = input.clone();
        let cancellation = cancelled.clone();
        std::thread::spawn(move || {
            match events.recv_timeout(first_frame_timeout) {
                Ok(Notice::Shown) => while matches!(events.recv(), Ok(Notice::Shown)) {},
                Ok(Notice::Retire) => {}
                Err(error) => {
                    if !cancellation.load(Ordering::Acquire) {
                        tracing::warn!("startup screen did not draw: {error}");
                    }
                }
            }
            transport.lock().expect("startup input lock").take();
            process.stop(EXIT_GRACE);
        });
        Ok(Self {
            child,
            input,
            cancelled,
            lifetime,
        })
    }

    /// Reports a real startup phase; transport failure leaves the normal client lifecycle intact.
    pub fn progress(&self, stage: Stage) {
        self.send(Message::Progress { stage });
    }

    /// Releases feedback after the destination screen has completed rendering.
    pub fn ready(&self) {
        self.send(Message::Ready);
        let _ = self.lifetime.send(Notice::Retire);
    }

    /// Whether the user closed the loading window before its destination was ready.
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Writes one bounded protocol message, preserving order across startup callbacks.
    fn send(&self, message: Message) {
        if let Some(input) = self.input.lock().expect("startup input lock").as_mut() {
            let _ = serde_json::to_writer(&mut *input, &message);
            let _ = input.write_all(b"\n");
            let _ = input.flush();
        }
    }
}

impl Drop for Process {
    /// EOF ends a responsive helper; killing and reaping also handles a stalled display driver.
    fn drop(&mut self) {
        let _ = self.lifetime.send(Notice::Retire);
        self.input.lock().expect("startup input lock").take();
        self.child.stop(EXIT_GRACE);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn startup_can_send_progress_before_the_loading_window_draws() {
        let directory = tempfile::tempdir().unwrap();
        let handoff = directory.path().join("handoff.json");
        // The helper cannot draw until startup has continued far enough to send progress.
        let child = children::spawn(
            Command::new("sh")
                .args([
                    "-c",
                    "read -r progress; printf '\"shown\"\\n\"cancelled\"\\n'; read -r ready; printf '%s' \"$ready\" > \"$1\"",
                    "startup-fixture",
                ])
                .arg(&handoff)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped()),
        )
        .unwrap();
        let controller = Process::from_child(child, FIRST_FRAME_TIMEOUT).unwrap();
        controller.progress(Stage::PreparingInterface);
        test_time::eventually("loading window cancellation", || controller.cancelled());
        controller.ready();
        controller.child.wait();
        let message: Message = serde_json::from_slice(&std::fs::read(handoff).unwrap()).unwrap();
        assert_eq!(message, Message::Ready);
    }

    /// Starts a helper that ignores protocol input and graceful termination, like a stuck driver.
    fn unresponsive_helper() -> Spawned {
        children::spawn(
            Command::new("sh")
                .args(["-c", "trap '' TERM; while :; do :; done"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped()),
        )
        .unwrap()
    }

    #[test]
    fn unresponsive_helper_is_reaped_after_its_first_frame_deadline() {
        let controller = Process::from_child(unresponsive_helper(), Duration::ZERO).unwrap();
        test_time::eventually("timed-out loading helper reaped", || {
            controller.child.try_wait().unwrap().is_some()
        });
        assert!(!controller.cancelled());
    }

    #[test]
    fn unresponsive_helper_is_reaped_when_content_is_ready_before_it_draws() {
        // Keep the watchdog beyond this test's wait so only readiness can reap the helper.
        let controller =
            Process::from_child(unresponsive_helper(), test_time::DEFAULT_TIMEOUT * 2).unwrap();
        controller.ready();
        test_time::eventually("retired loading helper reaped", || {
            controller.child.try_wait().unwrap().is_some()
        });
        assert!(!controller.cancelled());
    }
}
