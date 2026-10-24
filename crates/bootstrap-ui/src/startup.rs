//! Delayed startup feedback keeps fast launches free of a second process.

mod process;
mod window;

use anyhow::{Context, Result};
use launcher::startup::Stage;
use std::{
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

/// Selects the loading-window process before ordinary client argument parsing.
pub const FLAG: &str = "--startup-screen";
/// Fast launches finish before the feedback process is needed.
pub const SHOW_DELAY: Duration = Duration::from_millis(300);

/// Owns delayed feedback and retires it on every return or unwind from startup.
pub struct Controller(Arc<Shared>);

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

struct State {
    retired: bool,
    stage: Stage,
    process: Option<process::Process>,
}

impl Controller {
    /// Schedules feedback without creating a helper until the show delay expires.
    pub fn start(size: [f32; 2]) -> Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                retired: false,
                stage: Stage::LoadingAssets,
                process: None,
            }),
            wake: Condvar::new(),
        });
        let pending = shared.clone();
        let deadline = Instant::now() + SHOW_DELAY;
        std::thread::Builder::new()
            .name("startup-delay".into())
            .spawn(move || {
                let state = pending.state.lock().expect("startup delay lock");
                let (mut state, _) = pending
                    .wake
                    .wait_timeout_while(
                        state,
                        deadline.saturating_duration_since(Instant::now()),
                        |state| !state.retired,
                    )
                    .expect("startup delay lock");
                state.show(|| process::Process::start(size));
            })
            .context("schedule startup feedback")?;
        Ok(Self(shared))
    }

    /// Retains the latest phase so delayed feedback opens at the actual startup stage.
    pub fn progress(&self, stage: Stage) {
        let mut state = self.0.state.lock().expect("startup delay lock");
        state.stage = stage;
        if let Some(process) = &state.process {
            process.progress(stage);
        }
    }

    /// Prevents a pending spawn or retires an already running helper.
    pub fn ready(&self) {
        let mut state = self.0.state.lock().expect("startup delay lock");
        state.retired = true;
        if let Some(process) = &state.process {
            process.ready();
        }
        self.0.wake.notify_all();
    }

    /// Reports cancellation only after a helper actually opened.
    pub fn cancelled(&self) -> bool {
        self.0
            .state
            .lock()
            .expect("startup delay lock")
            .process
            .as_ref()
            .is_some_and(process::Process::cancelled)
    }
}

impl State {
    /// The same locked admission fence handles readiness both before and at the deadline.
    fn show(&mut self, spawn: impl FnOnce() -> Result<process::Process>) {
        if self.retired {
            return;
        }
        match spawn() {
            Ok(process) => {
                process.progress(self.stage);
                self.process = Some(process);
            }
            Err(error) => tracing::warn!("startup feedback unavailable: {error:#}"),
        }
    }
}

impl Drop for Controller {
    /// Cancels the pending timer and reaps only this controller's process.
    fn drop(&mut self) {
        self.ready();
    }
}

/// Runs the loading process without game carriers or the client event loop.
pub fn run() -> Result<()> {
    window::run()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_finished_before_the_show_delay_never_spawns_feedback() {
        let controller = Controller(Arc::new(Shared {
            state: Mutex::new(State {
                retired: false,
                stage: Stage::LoadingAssets,
                process: None,
            }),
            wake: Condvar::new(),
        }));
        controller.progress(Stage::WaitingForContent);
        controller.ready();
        let mut spawned = 0;
        controller.0.state.lock().unwrap().show(|| {
            spawned += 1;
            anyhow::bail!("fixture must stay unspawned")
        });
        assert_eq!(spawned, 0);
        assert!(!controller.cancelled());
    }

    #[test]
    fn unfinished_startup_attempts_feedback_once_at_the_deadline() {
        let mut state = State {
            retired: false,
            stage: Stage::PreparingInterface,
            process: None,
        };
        let mut spawned = 0;
        state.show(|| {
            spawned += 1;
            anyhow::bail!("fixture spawn failure")
        });
        assert_eq!(spawned, 1);
        state.retired = true;
        state.show(|| panic!("retired startup cannot retry a helper"));
    }
}
