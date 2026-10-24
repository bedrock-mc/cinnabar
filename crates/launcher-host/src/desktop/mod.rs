//! OS shell integration through native APIs, keeping outside tools only as fallbacks.

use std::path::{Path, PathBuf};

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "linux")]
pub mod dbus;
pub mod picker;
#[cfg(windows)]
pub mod windows;
#[cfg(any(windows, test))]
mod windows_filter;

/// Whether this executable has no console for its errors: a Windows GUI-subsystem build.
pub fn gui_subsystem() -> bool {
    #[cfg(windows)]
    {
        windows::gui_subsystem()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Opens a trusted or user-confirmed web URL on a worker thread.
pub fn open_url(url: &str) {
    let url = url.to_owned();
    let spawned = std::thread::Builder::new()
        .name("open-url".into())
        .spawn(move || {
            if !open_with_default(&url) {
                tracing::warn!("no browser could open {url}");
            }
        });
    if let Err(error) = spawned {
        tracing::warn!("could not start the browser launcher: {error}");
    }
}

/// Whether `open_folder` has a file manager to hand folders to on this OS.
pub fn opens_folders() -> bool {
    is_desktop(std::env::consts::OS)
}

/// Whether the game window receives files dragged onto it on this OS.
pub fn accepts_file_drops() -> bool {
    is_desktop(std::env::consts::OS)
}

/// Desktop systems have a file manager and windows that take dropped files. Android and other
/// targets have neither, so they get no folder buttons and import only through the picker.
fn is_desktop(os: &str) -> bool {
    matches!(os, "linux" | "macos" | "windows")
}

/// Creates `dir` if needed and shows it in the OS file manager on a worker thread.
pub fn open_folder(dir: PathBuf) {
    if !opens_folders() {
        tracing::warn!(
            "this platform has no file manager to open {}",
            dir.display()
        );
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("open-folder".into())
        .spawn(move || {
            if let Err(error) = std::fs::create_dir_all(&dir) {
                tracing::warn!("could not create {}: {error}", dir.display());
            } else if !show_folder(&dir) {
                tracing::warn!("no file manager could open {}", dir.display());
            }
        });
    if let Err(error) = spawned {
        tracing::warn!("could not start the file manager launcher: {error}");
    }
}

/// Prefers the file manager's own D-Bus service on Linux, then the desktop opener.
fn show_folder(dir: &Path) -> bool {
    #[cfg(windows)]
    return windows::shell_open(dir);
    #[cfg(target_os = "linux")]
    return dbus::show_folder(dir)
        .inspect_err(|error| {
            tracing::debug!("FileManager1 unavailable ({error}); trying {OPENER}");
        })
        .is_ok()
        || run_tool(dir);
    #[cfg(target_os = "macos")]
    return run_tool(dir);
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = dir;
        false
    }
}

/// Opens `url` in the desktop's default handler; cancellation suppresses further handlers.
pub fn open_with_default(url: &str) -> bool {
    !matches!(open_outcome(url), OpenOutcome::Failed)
}

/// Reports whether sign-in reached a browser, so a dismissed chooser keeps manual recovery visible.
pub fn open_sign_in_link(url: &str) -> bool {
    matches!(open_outcome(url), OpenOutcome::Opened)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OpenOutcome {
    Opened,
    #[cfg(any(target_os = "linux", test))]
    Cancelled,
    Failed,
}

/// Shares platform routing while preserving cancellation separately from handoff success.
fn open_outcome(url: &str) -> OpenOutcome {
    #[cfg(windows)]
    return handoff_result(windows::shell_open(url));
    #[cfg(target_os = "linux")]
    return portal_outcome(dbus::open_uri(url), || run_tool(url));
    #[cfg(target_os = "android")]
    return handoff_result(android::open_url(url).unwrap_or_else(|error| {
        tracing::warn!("the activity could not open a browser: {error}");
        false
    }));
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
    return handoff_result(run_tool(url));
}

/// Only unavailable or failed portal requests may try another desktop handler.
#[cfg(any(target_os = "linux", test))]
fn portal_outcome(
    result: Result<bool, impl std::fmt::Display>,
    fallback: impl FnOnce() -> bool,
) -> OpenOutcome {
    match result {
        Ok(true) => OpenOutcome::Opened,
        Ok(false) => OpenOutcome::Cancelled,
        Err(error) => {
            tracing::debug!("portal OpenURI unavailable ({error}); trying xdg-open");
            handoff_result(fallback())
        }
    }
}

/// Converts an OS handler's completion into the shared handoff outcome.
fn handoff_result(opened: bool) -> OpenOutcome {
    if opened {
        OpenOutcome::Opened
    } else {
        OpenOutcome::Failed
    }
}

/// The desktop's command-line opener for URLs and paths.
#[cfg(target_os = "macos")]
const OPENER: &str = "open";
#[cfg(all(unix, not(any(target_os = "macos", target_os = "android"))))]
const OPENER: &str = "xdg-open";

#[cfg(all(unix, not(target_os = "android")))]
fn run_tool(argument: impl AsRef<std::ffi::OsStr>) -> bool {
    opener(argument)
        .status()
        .is_ok_and(|status| status.success())
}

/// Passes `argument` as one argv entry, never through a shell.
#[cfg(all(unix, not(target_os = "android")))]
fn opener(argument: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    use std::process::{Command, Stdio};
    let mut command = Command::new(OPENER);
    command
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_buttons_and_file_drops_exist_only_on_desktop_systems() {
        for os in ["linux", "macos", "windows"] {
            assert!(is_desktop(os), "{os}");
        }
        for os in ["android", "ios", "freebsd"] {
            assert!(!is_desktop(os), "{os}");
        }
    }

    #[cfg(all(unix, not(target_os = "android")))]
    #[test]
    fn the_opener_gets_the_folder_as_one_unparsed_argument() {
        let dir = Path::new("/data/My Worlds; echo $HOME");
        let command = opener(dir);
        let expected = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        assert_eq!(command.get_program(), expected);
        assert_eq!(command.get_args().collect::<Vec<_>>(), [dir.as_os_str()]);
    }

    #[test]
    fn cancelled_portal_handoff_keeps_sign_in_recovery_without_fallback() {
        let result = portal_outcome(Ok::<_, &str>(false), || {
            panic!("cancelled chooser tried another handler")
        });
        assert_eq!(result, OpenOutcome::Cancelled);
        assert!(!matches!(result, OpenOutcome::Opened));
        assert!(!matches!(result, OpenOutcome::Failed));
    }

    #[test]
    fn successful_portal_handoff_never_uses_a_fallback() {
        assert_eq!(
            portal_outcome(Ok::<_, &str>(true), || panic!(
                "successful portal tried another handler"
            )),
            OpenOutcome::Opened
        );
    }

    #[test]
    fn unavailable_portal_uses_the_injected_handler_result() {
        for (opened, expected) in [(true, OpenOutcome::Opened), (false, OpenOutcome::Failed)] {
            let mut calls = 0;
            assert_eq!(
                portal_outcome(Err("portal unavailable"), || {
                    calls += 1;
                    opened
                }),
                expected
            );
            assert_eq!(calls, 1);
        }
    }
}
