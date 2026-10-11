//! OS shell integration through native APIs, keeping outside tools only as fallbacks.

#[cfg(target_os = "linux")]
pub mod dbus;
#[cfg(windows)]
pub mod windows;

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
    #[cfg(target_os = "macos")]
    return handoff_result(run_tool("open", url));
    #[cfg(target_os = "linux")]
    return portal_outcome(dbus::open_uri(url), || run_tool("xdg-open", url));
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    return handoff_result(run_tool("xdg-open", url));
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

#[cfg(test)]
mod tests {
    use super::*;

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
