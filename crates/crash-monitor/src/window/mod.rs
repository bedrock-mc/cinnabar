//! The crash report window: what happened, where the report is, and what to do next. It draws
//! with the setup window's panorama, panel, type and controls, so it needs no game assets.

mod app;
mod desktop;
mod discord;
mod summary;
mod view;

pub use summary::failure_title;

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use bootstrap_ui::{Preferences, canvas::Text, input::Actions};
use winit::{
    dpi::LogicalSize,
    event_loop::{ControlFlow, EventLoop},
};

use launcher::install_layout::InstallLayout;

/// The crash window's default logical size.
const CRASH_SIZE: LogicalSize<f64> = LogicalSize::new(960.0, 640.0);
/// How long Copy report shows its outcome before reading "Copy report" again.
const COPY_FEEDBACK: Duration = Duration::from_secs(2);

/// What one crash report says, as the crash window shows it.
#[derive(Clone, Debug)]
pub struct CrashSummary {
    /// Which failure this is, which sets the title and the opening sentence.
    pub headline: Headline,
    /// The fault, such as an exception code and address, or the error that stopped startup. Its
    /// first line fills the summary row; a longer error is shown whole in the details.
    pub fault: String,
    /// The faulting module offset and thread.
    pub location: String,
    pub version: String,
    pub graphics: String,
    pub session: String,
    /// The client log's last lines, oldest first.
    pub log_tail: Vec<String>,
    /// The last packets before the crash, oldest first; empty leaves the section out.
    pub last_packets: Vec<String>,
    /// The report folder on this computer.
    pub folder: PathBuf,
}

/// How the client failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Headline {
    /// The game stopped while running; `activity` completes "while you were …".
    Crashed { activity: String },
    /// `main` failed before the game started.
    CouldNotStart,
    /// `main` failed on an error after the game started, while the player was `activity`.
    Stopped { activity: String },
}

/// What the player chose in the crash window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Choice {
    /// Start the game again; the caller relaunches it.
    Restart,
    /// Close, Escape or closing the window.
    Close,
    /// No font, event loop, window or GPU adapter: no window could open.
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CrashAction {
    Restart,
    CopyReport,
    OpenFolder,
    Close,
    ToggleDetails,
    Discord,
}

/// Focus order: the action row first so Restart is the Enter default, then the row above it
/// from right to left, so Close moves up to Discord and Restart moves back up to the toggle.
const ACTIONS: [CrashAction; 6] = [
    CrashAction::Restart,
    CrashAction::CopyReport,
    CrashAction::OpenFolder,
    CrashAction::Close,
    CrashAction::Discord,
    CrashAction::ToggleDetails,
];

/// The action row, left to right.
const ROW: [CrashAction; 4] = [
    CrashAction::Restart,
    CrashAction::CopyReport,
    CrashAction::OpenFolder,
    CrashAction::Close,
];

/// The result Copy report shows on its button until it expires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Copied {
    Done,
    Failed,
}

/// The window's interactive state.
struct CrashScreen {
    expanded: bool,
    copied: Option<(Copied, Instant)>,
}

impl Actions for CrashScreen {
    type Action = CrashAction;

    fn actions(&self) -> &[CrashAction] {
        &ACTIONS
    }

    fn cancel(&self) -> CrashAction {
        CrashAction::Close
    }
}

impl CrashScreen {
    fn label(&self, action: CrashAction) -> &'static str {
        match action {
            CrashAction::Restart => "Restart",
            CrashAction::CopyReport => match self.copied {
                Some((Copied::Done, _)) => "Copied",
                Some((Copied::Failed, _)) => "Copy failed",
                None => "Copy report",
            },
            CrashAction::OpenFolder => "Open folder",
            CrashAction::Close => "Close",
            CrashAction::ToggleDetails if self.expanded => "Hide details",
            CrashAction::ToggleDetails => "Show details",
            CrashAction::Discord => "Join our Discord",
        }
    }

    /// Clears expired Copy report feedback; true when the button label changed.
    fn expire(&mut self, now: Instant) -> bool {
        let expired = self
            .copied
            .is_some_and(|(_, at)| now.duration_since(at) >= COPY_FEEDBACK);
        if expired {
            self.copied = None;
        }
        expired
    }
}

/// Puts `summary`'s Markdown issue text on the clipboard, as the Copy report button does; the
/// text names no report folder and writes the player's home directory as `~`.
pub(crate) fn copy_report(summary: &CrashSummary) -> Result<(), String> {
    desktop::copy_text(&summary.issue_markdown())
}

/// Shows the crash window for `summary` with the UI font and menu preferences `layout` holds, and
/// waits for the player's choice. [`Choice::Unavailable`] means no font, event loop, window or
/// GPU adapter, so the caller should fall back to a native dialog.
pub fn show(summary: CrashSummary, layout: &InstallLayout) -> Choice {
    match bootstrap_ui::text(layout) {
        Ok(text) => show_with_text(summary, text, Preferences::load(layout)),
        Err(error) => {
            tracing::warn!("crash window unavailable: no usable UI font ({error:#})");
            Choice::Unavailable
        }
    }
}

/// [`show`] with caller-supplied UI font bytes (Cinnangles Sans) and preferences.
pub fn show_with_font(summary: CrashSummary, font: &[u8], preferences: Preferences) -> Choice {
    match Text::from_bytes(font) {
        Some(text) => show_with_text(summary, text, preferences),
        None => {
            tracing::warn!("crash window unavailable: the UI font does not parse");
            Choice::Unavailable
        }
    }
}

fn show_with_text(summary: CrashSummary, text: Text, preferences: Preferences) -> Choice {
    let Some(faces) = bootstrap_ui::built_in_faces() else {
        return Choice::Unavailable;
    };
    let Ok(event_loop) = EventLoop::new() else {
        return Choice::Unavailable;
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let logo = bootstrap_ui::decode_logo();
    let mut app = app::CrashApp::new(summary, text, faces, logo, preferences);
    let failed = event_loop.run_app(&mut app).is_err();
    app.choice().unwrap_or(if failed {
        Choice::Unavailable
    } else {
        Choice::Close
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_feedback_expires_back_to_its_label() {
        let at = Instant::now();
        let mut screen = CrashScreen {
            expanded: false,
            copied: Some((Copied::Done, at)),
        };
        assert_eq!(screen.label(CrashAction::CopyReport), "Copied");
        assert!(!screen.expire(at));
        assert!(screen.expire(at + COPY_FEEDBACK));
        assert_eq!(screen.label(CrashAction::CopyReport), "Copy report");
    }
}
