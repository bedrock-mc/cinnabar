//! OS shell integration through native APIs, keeping outside tools only as fallbacks.

#[cfg(target_os = "linux")]
pub(crate) mod dbus;
#[cfg(windows)]
pub(crate) mod windows;

/// Opens a trusted or user-confirmed web URL on a worker thread.
pub(crate) fn open_url(url: &str) {
    let url = url.to_owned();
    let spawned = std::thread::Builder::new()
        .name("open-url".into())
        .spawn(move || {
            if !open_with_default(&url) {
                bevy::log::warn!("no browser could open {url}");
            }
        });
    if let Err(error) = spawned {
        bevy::log::warn!("could not start the browser launcher: {error}");
    }
}

/// Opens `url` in the desktop's default handler and blocks until it is handed off.
pub(crate) fn open_with_default(url: &str) -> bool {
    #[cfg(windows)]
    return windows::shell_open(url);
    #[cfg(target_os = "macos")]
    return run_tool("open", url);
    #[cfg(target_os = "linux")]
    return match dbus::open_uri(url) {
        Ok(()) => true,
        Err(error) => {
            bevy::log::debug!("portal OpenURI unavailable ({error}); trying xdg-open");
            run_tool("xdg-open", url)
        }
    };
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    return run_tool("xdg-open", url);
}

#[cfg(unix)]
fn run_tool(program: &str, argument: &str) -> bool {
    use std::process::{Command, Stdio};
    Command::new(program)
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
