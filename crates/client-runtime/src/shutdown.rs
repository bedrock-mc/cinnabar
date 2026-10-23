//! Bounded process-shutdown watchdog shared by startup and teardown.

use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    thread,
    time::Duration,
};

use bevy::{
    app::AppExit,
    prelude::{MessageReader, Res, Resource},
};

use diagnostics::markers::{SHUTDOWN_WATCHDOG_ARMED_MARKER, SHUTDOWN_WATCHDOG_FIRED_MARKER};

pub const SHUTDOWN_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(2);

pub const SHUTDOWN_WATCHDOG_IDLE: u8 = 0;
pub const SHUTDOWN_WATCHDOG_ARMED: u8 = 1;
pub const SHUTDOWN_WATCHDOG_COMPLETED: u8 = 2;
pub const SHUTDOWN_WATCHDOG_FIRED: u8 = 3;

pub type ShutdownTerminator = Arc<dyn Fn(i32) + Send + Sync + 'static>;

#[derive(Resource, Clone)]
pub struct ShutdownWatchdog {
    pub state: Arc<AtomicU8>,
    pub timeout: Duration,
    pub terminate: ShutdownTerminator,
}

impl ShutdownWatchdog {
    /// Creates a watchdog that ends the client as `main` would after a finished teardown.
    pub fn process(timeout: Duration) -> Self {
        Self::new(timeout, |code| {
            end_cut_short(
                code,
                || launcher_host::lifecycle::children::stop_all(Duration::ZERO),
                launcher_host::lifecycle::update::apply_after_exit,
                launcher_host::lifecycle::crash::announce_exit,
            );
            std::process::exit(code)
        })
    }

    /// Creates a watchdog with a caller-supplied termination action.
    pub fn new<F>(timeout: Duration, terminate: F) -> Self
    where
        F: Fn(i32) + Send + Sync + 'static,
    {
        Self {
            state: Arc::new(AtomicU8::new(SHUTDOWN_WATCHDOG_IDLE)),
            timeout,
            terminate: Arc::new(terminate),
        }
    }

    /// Starts the timeout once and returns whether this call armed it.
    pub fn arm(&self, exit: AppExit) -> bool {
        if self
            .state
            .compare_exchange(
                SHUTDOWN_WATCHDOG_IDLE,
                SHUTDOWN_WATCHDOG_ARMED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return false;
        }
        let state = Arc::clone(&self.state);
        let terminate = Arc::clone(&self.terminate);
        let timeout = self.timeout;
        let exit_code = app_exit_code(&exit);
        let spawned = thread::Builder::new()
            .name("bedrock-shutdown-watchdog".to_owned())
            .spawn(move || {
                thread::sleep(timeout);
                if state
                    .compare_exchange(
                        SHUTDOWN_WATCHDOG_ARMED,
                        SHUTDOWN_WATCHDOG_FIRED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    let _ = writeln!(
                        diagnostics::console::stderr(),
                        "{SHUTDOWN_WATCHDOG_FIRED_MARKER} timeout_ms={} exit_code={exit_code}",
                        timeout.as_millis()
                    );
                    diagnostics::console::flush_before_exit();
                    terminate(exit_code);
                }
            });
        if spawned.is_err() {
            self.state.store(SHUTDOWN_WATCHDOG_FIRED, Ordering::Release);
            diagnostics::console::flush_before_exit();
            (self.terminate)(exit_code);
        }
        true
    }

    /// Marks shutdown complete so an armed timeout cannot terminate the process.
    pub fn complete(&self) {
        self.state
            .store(SHUTDOWN_WATCHDOG_COMPLETED, Ordering::Release);
    }
}

/// Runs the exit steps a teardown cut short by the watchdog would otherwise skip: `stop` ends
/// owned children, a successful exit still runs `hand_off_update` so an accepted update reaches
/// its helper, and `announce` reports the status to the crash monitor last.
fn end_cut_short(
    code: i32,
    stop: impl FnOnce(),
    hand_off_update: impl FnOnce(),
    announce: impl FnOnce(i32),
) {
    stop();
    if code == 0 {
        hand_off_update();
    }
    announce(code);
}

/// Arms the watchdog when the world is torn down without an `AppExit`, as macOS `terminate:`
/// (Cmd+Q, Dock Quit) and a panic unwinding the main thread do; Bevy's pipelined-render teardown
/// otherwise deadlocks there. Insert before the render plugins finish so it drops ahead of
/// `RenderAppChannels`.
#[derive(Resource)]
pub struct TeardownWatchdog(pub ShutdownWatchdog);

/// The status Rust exits with after a panic on the main thread.
const PANIC_EXIT_CODE: u8 = launcher_host::lifecycle::crash::protocol::PANIC_EXIT_CODE;

impl Drop for TeardownWatchdog {
    fn drop(&mut self) {
        // A watchdog cutting short a panic's teardown must still end the process as failed.
        let exit = if thread::panicking() {
            AppExit::from_code(PANIC_EXIT_CODE)
        } else {
            AppExit::Success
        };
        begin_bounded_shutdown(&self.0, &exit);
    }
}

/// Converts the Bevy exit reason to its process exit status.
pub fn app_exit_code(exit: &AppExit) -> i32 {
    match exit {
        AppExit::Success => 0,
        AppExit::Error(code) => i32::from(code.get()),
    }
}

/// Arms shutdown once and records the requested timeout and exit status.
pub fn begin_bounded_shutdown(watchdog: &ShutdownWatchdog, exit: &AppExit) {
    if watchdog.arm(exit.clone()) {
        let _ = writeln!(
            diagnostics::console::stderr(),
            "{SHUTDOWN_WATCHDOG_ARMED_MARKER} timeout_ms={} exit_code={}",
            watchdog.timeout.as_millis(),
            app_exit_code(exit)
        );
    }
}

/// Arms the watchdog for exit requests, preserving any failure status.
pub fn arm_shutdown_watchdog(mut exits: MessageReader<AppExit>, watchdog: Res<ShutdownWatchdog>) {
    let requested = exits.read().cloned().reduce(
        |selected, next| {
            if selected.is_error() { selected } else { next }
        },
    );
    if let Some(exit) = requested {
        begin_bounded_shutdown(&watchdog, &exit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn cut_short(code: i32) -> Vec<String> {
        let steps = RefCell::new(Vec::new());
        end_cut_short(
            code,
            || steps.borrow_mut().push("stop".to_owned()),
            || steps.borrow_mut().push("hand off update".to_owned()),
            |code| steps.borrow_mut().push(format!("announce {code}")),
        );
        steps.into_inner()
    }

    #[test]
    fn a_successful_exit_cut_short_still_hands_off_an_accepted_update() {
        assert_eq!(cut_short(0), ["stop", "hand off update", "announce 0"]);
    }

    #[test]
    fn a_failed_exit_cut_short_never_installs_an_update() {
        assert_eq!(cut_short(1), ["stop", "announce 1"]);
        assert_eq!(
            cut_short(i32::from(PANIC_EXIT_CODE)),
            ["stop", format!("announce {PANIC_EXIT_CODE}").as_str()]
        );
    }
}
