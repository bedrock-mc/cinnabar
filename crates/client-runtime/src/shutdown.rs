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
    /// Creates a watchdog that stops owned child processes before terminating the client.
    pub fn process(timeout: Duration) -> Self {
        Self::new(timeout, |code| {
            launcher_host::lifecycle::children::stop_all(std::time::Duration::ZERO);
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

/// Arms the watchdog when the world is torn down without an `AppExit`, as macOS `terminate:`
/// (Cmd+Q, Dock Quit) does; Bevy's pipelined-render teardown otherwise deadlocks there.
/// Insert before the render plugins finish so it drops ahead of `RenderAppChannels`.
#[derive(Resource)]
pub struct TeardownWatchdog(pub ShutdownWatchdog);

impl Drop for TeardownWatchdog {
    fn drop(&mut self) {
        begin_bounded_shutdown(&self.0, &AppExit::Success);
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
