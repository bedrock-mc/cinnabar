//! Responsive startup feedback in a process independent of the game's event loop.

mod window;

use anyhow::{Context, Result};
use launcher::startup::{Event, Message, Stage};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
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
    child: Mutex<Child>,
    input: Mutex<Option<ChildStdin>>,
    cancelled: Arc<AtomicBool>,
}

impl Controller {
    /// Starts feedback and waits briefly for its first submitted frame. An unavailable display
    /// returns an error so callers can continue with their normal startup diagnostics.
    pub fn start(size: [f32; 2]) -> Result<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .arg(FLAG)
            .args(size.map(|value| value.to_string()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("open startup screen")?;
        let input = child.stdin.take();
        let output = child.stdout.take().context("startup screen output")?;
        let cancelled = Arc::new(AtomicBool::new(false));
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
        let controller = Self {
            child: Mutex::new(child),
            input: Mutex::new(input),
            cancelled,
        };
        if let Err(error) = first_frame.recv_timeout(Duration::from_secs(5)) {
            if !controller.cancelled() {
                return Err(error).context("startup screen did not draw");
            }
        }
        Ok(controller)
    }

    /// Reports a real startup phase; transport failure leaves the normal client lifecycle intact.
    pub fn progress(&self, stage: Stage) {
        self.send(Message::Progress { stage });
    }

    /// Releases feedback after the destination screen has completed rendering.
    pub fn ready(&self) {
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
        self.input.get_mut().expect("startup input lock").take();
        let child = self.child.get_mut().expect("startup child lock");
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}

/// Runs the loading process, with no game carriers or client event loop required.
pub fn run() -> Result<()> {
    window::run()
}
