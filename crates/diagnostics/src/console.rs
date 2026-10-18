//! Ordered console markers through the platform's output owner.
#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod native;
#[cfg(target_arch = "wasm32")]
pub use browser::{Writer, stderr, stdout};
#[cfg(not(target_arch = "wasm32"))]
pub use native::{
    Console, Priority, Queued, SessionFiles, Stream, console, defer_stderr,
    drain_raw_stderr_on_signal, flush, flush_before_exit, initialize_log, stderr, stdout,
};
