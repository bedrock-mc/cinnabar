#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod entry;

/// Runs the client through its shared startup and exit dispatch.
fn main() {
    entry::dispatch();
}
