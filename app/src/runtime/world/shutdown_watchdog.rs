//! Bounded process-shutdown watchdog shared by production startup and tests.
//!
//! Extracted verbatim from the world-stream runtime module to respect the
//! per-file architecture line policy; behavior is unchanged and every public
//! path (`crate::runtime::world::ShutdownWatchdog` and friends) is preserved
//! through the world-module re-export.

use std::{
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

use crate::acceptance::markers::{SHUTDOWN_WATCHDOG_ARMED_MARKER, SHUTDOWN_WATCHDOG_FIRED_MARKER};

pub(crate) const SHUTDOWN_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) const SHUTDOWN_WATCHDOG_IDLE: u8 = 0;
pub(crate) const SHUTDOWN_WATCHDOG_ARMED: u8 = 1;
pub(crate) const SHUTDOWN_WATCHDOG_COMPLETED: u8 = 2;
pub(crate) const SHUTDOWN_WATCHDOG_FIRED: u8 = 3;

pub(crate) type ShutdownTerminator = Arc<dyn Fn(i32) + Send + Sync + 'static>;

#[derive(Resource, Clone)]
pub(crate) struct ShutdownWatchdog {
    pub(crate) state: Arc<AtomicU8>,
    pub(crate) timeout: Duration,
    pub(crate) terminate: ShutdownTerminator,
}

impl ShutdownWatchdog {
    pub(crate) fn process(timeout: Duration) -> Self {
        Self::new(timeout, |code| std::process::exit(code))
    }

    pub(crate) fn new<F>(timeout: Duration, terminate: F) -> Self
    where
        F: Fn(i32) + Send + Sync + 'static,
    {
        Self {
            state: Arc::new(AtomicU8::new(SHUTDOWN_WATCHDOG_IDLE)),
            timeout,
            terminate: Arc::new(terminate),
        }
    }

    pub(crate) fn arm(&self, exit: AppExit) -> bool {
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
                    eprintln!(
                        "{SHUTDOWN_WATCHDOG_FIRED_MARKER} timeout_ms={} exit_code={exit_code}",
                        timeout.as_millis()
                    );
                    terminate(exit_code);
                }
            });
        if spawned.is_err() {
            self.state.store(SHUTDOWN_WATCHDOG_FIRED, Ordering::Release);
            (self.terminate)(exit_code);
        }
        true
    }

    pub(crate) fn complete(&self) {
        self.state
            .store(SHUTDOWN_WATCHDOG_COMPLETED, Ordering::Release);
    }
}

pub(crate) fn app_exit_code(exit: &AppExit) -> i32 {
    match exit {
        AppExit::Success => 0,
        AppExit::Error(code) => i32::from(code.get()),
    }
}

pub(crate) fn begin_bounded_shutdown(watchdog: &ShutdownWatchdog, exit: &AppExit) {
    if watchdog.arm(exit.clone()) {
        eprintln!(
            "{SHUTDOWN_WATCHDOG_ARMED_MARKER} timeout_ms={} exit_code={}",
            watchdog.timeout.as_millis(),
            app_exit_code(exit)
        );
    }
}

pub(crate) fn arm_shutdown_watchdog(
    mut exits: MessageReader<AppExit>,
    watchdog: Res<ShutdownWatchdog>,
) {
    let requested = exits.read().cloned().reduce(
        |selected, next| {
            if selected.is_error() { selected } else { next }
        },
    );
    if let Some(exit) = requested {
        begin_bounded_shutdown(&watchdog, &exit);
    }
}
