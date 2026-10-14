//! The crash monitor process: it watches the game client and, when the client dies, writes the
//! crash folder (minidump, report and log sections) and shows what happened in the crash
//! window, on the same pre-game window stack as first-run setup.

mod actions;
mod describe;
mod dump;
mod outcome;
mod parent;
mod run;
mod server;
pub mod window;

pub use run::{Args, CRASH_UI_ENV, run};
