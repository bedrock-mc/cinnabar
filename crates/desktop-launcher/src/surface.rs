//! The command and event surface the web UI relies on.

/// The command and event surface a web UI bundle may rely on; bump it whenever a command,
/// event or payload changes, so a newer UI never runs against an older native launcher.
pub const NATIVE_API_VERSION: u32 = 1;

pub const UPDATE_EVENT: &str = "update-state";
pub const GAME_EVENT: &str = "game-state";
/// Every event the UI listens to.
#[cfg(test)]
pub const EVENTS: &[&str] = &[UPDATE_EVENT, GAME_EVENT];
/// The launcher's main window label, shared with the capability file.
pub const MAIN_WINDOW: &str = "main";
