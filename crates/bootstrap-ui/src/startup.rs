//! Responsive startup feedback in a process independent of the game's event loop.

mod window;

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
pub const FLAG: &str = "--startup-screen";

/// Owns the loading process and closes it on every return or unwind from client startup.
pub struct Controller {
    child: Spawned,
    input: Mutex<Option<ChildStdin>>,
    cancelled: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

impl Controller {
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
        Self::from_child(child)
    }

    /// Connects the feedback protocol to an already owned helper process.
    fn from_child(child: Spawned) -> Result<Self> {
        let input = child.take_stdin();
        let output = child.take_stdout().context("startup screen output")?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let (shown, first_frame) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                match serde_json::from_str::<Event>(&line) {
                    Ok(Event::Shown) => {
                        let _ = shown.send(());
                    }
                    Ok(Event::Cancelled) => {
                        flag.store(true, Ordering::Release);
                    }
                    Err(_) => {}
                }
            }
        });
        let retired = finished.clone();
        let cancellation = cancelled.clone();
        std::thread::spawn(move || {
            if let Err(error) = first_frame.recv_timeout(Duration::from_secs(5))
                && !retired.load(Ordering::Acquire)
                && !cancellation.load(Ordering::Acquire)
            {
                tracing::warn!("startup screen did not draw: {error}");
            }
        });
        Ok(Self {
            child,
            input: Mutex::new(input),
            cancelled,
            finished,
        })
    }

    /// Reports a real startup phase; transport failure leaves the normal client lifecycle intact.
    pub fn progress(&self, stage: Stage) {
        self.send(Message::Progress { stage });
    }

    /// Releases feedback after the destination screen has completed rendering.
    pub fn ready(&self) {
        self.finished.store(true, Ordering::Release);
        self.send(Message::Ready);
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

impl Drop for Controller {
    /// EOF ends a responsive helper; killing and reaping also handles a stalled display driver.
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Release);
        self.input.get_mut().expect("startup input lock").take();
        self.child.stop(EXIT_GRACE);
    }
}

/// Runs the loading process, with no game carriers or client event loop required.
pub fn run() -> Result<()> {
    window::run()
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
        let controller = Controller::from_child(child).unwrap();
        controller.progress(Stage::PreparingInterface);
        test_time::eventually("loading window cancellation", || controller.cancelled());
        controller.ready();
        controller.child.wait();
        let message: Message = serde_json::from_slice(&std::fs::read(handoff).unwrap()).unwrap();
        assert_eq!(message, Message::Ready);
    }
}
