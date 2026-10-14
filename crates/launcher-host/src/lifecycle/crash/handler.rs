//! Native fault capture through `crash-handler`. The callback runs on the faulting thread (in
//! a signal handler on Linux), so it only asks the monitor for a dump, falling back to the
//! prepared in-process report when no monitor is connected.

use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use crash_handler::{CrashContext, CrashEventResult, CrashHandler};

use super::{Context, LogHooks, monitor};

static HANDLER: OnceLock<CrashHandler> = OnceLock::new();
/// Set while the panic hook dumps its own thread, so the callback only requests that dump.
static PANIC_DUMP: AtomicBool = AtomicBool::new(false);
/// `EXCEPTION_STACK_OVERFLOW`: almost no stack is left for anything but the dump request.
#[cfg(windows)]
const STACK_OVERFLOW: i32 = 0xC000_00FD_u32 as i32;

/// Attaches the process crash handler once.
pub(super) fn install(context: &'static Context, hooks: LogHooks) {
    if HANDLER.get().is_some() {
        return;
    }
    // SAFETY: The callback only reads immutable statics, drains or flushes logs, sends the dump
    // request and writes the prepared fallback report.
    let event = unsafe {
        crash_handler::make_crash_event(move |crash: &CrashContext| on_crash(context, hooks, crash))
    };
    match CrashHandler::attach(event) {
        Ok(handler) => {
            let _ = HANDLER.set(handler);
        }
        Err(error) => tracing::warn!("native crash capture unavailable: {error}"),
    }
}

fn on_crash(context: &Context, hooks: LogHooks, crash: &CrashContext) -> CrashEventResult {
    if PANIC_DUMP.load(Ordering::Acquire) {
        return CrashEventResult::Handled(request_dump(context, crash));
    }
    if super::REPORTED.swap(true, Ordering::AcqRel) {
        return CrashEventResult::Handled(false);
    }
    // Output buffered for the client log must land before the monitor copies its section.
    #[cfg(unix)]
    (hooks.drain_on_signal)();
    #[cfg(windows)]
    if crash.exception_code != STACK_OVERFLOW {
        (hooks.flush)();
    }
    let _ = hooks;
    if request_dump(context, crash) {
        // The monitor reports it; on Windows that ends the process without the system's crash
        // dialog, elsewhere the previous handlers and the default action still run.
        return CrashEventResult::Handled(cfg!(windows));
    }
    #[cfg(windows)]
    super::fallback::write_native(context, crash);
    #[cfg(unix)]
    super::fallback::write_native(crash);
    CrashEventResult::Handled(false)
}

/// Asks the connected monitor to dump this process for `crash`; false without one.
fn request_dump(context: &Context, crash: &CrashContext) -> bool {
    let Some(client) = monitor::connection() else {
        return false;
    };
    #[cfg(windows)]
    if context.full_dump {
        let request = super::protocol::DumpContext {
            exception_pointers: crash.exception_pointers as u64,
            process_id: crash.process_id,
            thread_id: crash.thread_id,
            exception_code: crash.exception_code,
        };
        let _ = client.send_message(super::protocol::DUMP_CONTEXT, request.to_bytes());
    }
    let _ = context;
    client.request_dump(crash).is_ok()
}

/// Dumps the calling thread through the monitor, for a panic; false without a monitor.
pub(super) fn dump_this_thread() -> bool {
    let Some(handler) = HANDLER.get() else {
        return false;
    };
    if monitor::connection().is_none() {
        return false;
    }
    PANIC_DUMP.store(true, Ordering::Release);
    #[cfg(windows)]
    let dumped = matches!(
        handler.simulate_exception(None),
        CrashEventResult::Handled(true)
    );
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let dumped = matches!(
        handler.simulate_signal(libc::SIGABRT as u32),
        CrashEventResult::Handled(true)
    );
    #[cfg(target_os = "macos")]
    let dumped = handler.simulate_exception(None);
    PANIC_DUMP.store(false, Ordering::Release);
    dumped
}

/// Lets the monitor process `pid` read this process's memory during a crash, which Linux's
/// Yama policy otherwise denies to a child.
pub(super) fn allow_dumps_by(pid: u32) {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(handler) = HANDLER.get() {
        handler.set_ptracer(Some(pid));
    }
    let _ = pid;
}
