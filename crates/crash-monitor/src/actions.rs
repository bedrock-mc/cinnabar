//! What the player can do about a crash outside the crash window.

use std::process::{Command, Stdio};

use launcher_host::{
    lifecycle::crash::{self, protocol::Restart},
    native_dialog::{self, CrashChoice},
};

use crate::window::CrashSummary;

/// Starts the client again as the player first started it, detached from this process: this
/// executable, which is the client's, with the arguments the client handed over at launch.
pub(crate) fn restart(restart: &Restart) -> std::io::Result<()> {
    let mut command = Command::new(crash::restart_program()?);
    command
        .args(&restart.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for name in &restart.env_remove {
        command.env_remove(name);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().map(drop)
}

/// The crash window's choices in a native dialog, for when no window can open; true when the
/// player chose Restart.
pub(crate) fn native_dialog(summary: &CrashSummary) -> bool {
    let title = summary.title();
    let body = format!(
        "{}\n\nWhat happened: {}\nWhere: {}\n\nThe report is in {}",
        summary.body(),
        summary.fault,
        summary.location,
        summary.folder.display()
    );
    loop {
        match native_dialog::crash_choice(&title, &body) {
            CrashChoice::Restart => return true,
            CrashChoice::OpenFolder => {
                launcher_host::desktop::open_url(&summary.folder.display().to_string());
            }
            CrashChoice::CopyReport => {
                if let Err(error) = crate::window::copy_report(summary) {
                    tracing::warn!("copy report failed: {error}");
                }
            }
            CrashChoice::Close | CrashChoice::Unavailable => return false,
        }
    }
}
