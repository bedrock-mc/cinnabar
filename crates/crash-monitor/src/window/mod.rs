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
    SendReport,
    OpenFolder,
    Close,
    ToggleDetails,
    Discord,
}

/// Focus order: the action row first so Restart is the Enter default, then the row above it
/// from right to left, so Close moves up to Discord and Restart moves back up to the toggle.
const ACTIONS: [CrashAction; 7] = [
    CrashAction::Restart,
    CrashAction::CopyReport,
    CrashAction::SendReport,
    CrashAction::OpenFolder,
    CrashAction::Close,
    CrashAction::Discord,
    CrashAction::ToggleDetails,
];

/// The action row, left to right.
const ROW: [CrashAction; 5] = [
    CrashAction::Restart,
    CrashAction::CopyReport,
    CrashAction::SendReport,
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
    queued: bool,
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
            CrashAction::SendReport if self.queued => "Queued",
            CrashAction::SendReport => "Send report",
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
/// calls `selected` with the player's choice before waiting for pending uploads.
/// [`Choice::Unavailable`] means no font, event loop, window or
/// GPU adapter, so the caller should fall back to a native dialog.
pub fn show(
    summary: CrashSummary,
    layout: &InstallLayout,
    selected: impl FnOnce(Choice),
) -> Choice {
    match bootstrap_ui::text(layout) {
        Ok(text) => show_with_text(
            summary,
            text,
            Preferences::load(layout),
            Some(layout.clone()),
            selected,
        ),
        Err(error) => {
            tracing::warn!("crash window unavailable: no usable UI font ({error:#})");
            selected(Choice::Unavailable);
            Choice::Unavailable
        }
    }
}

/// [`show`] with caller-supplied UI font bytes (Cinnangles Sans) and preferences.
pub fn show_with_font(summary: CrashSummary, font: &[u8], preferences: Preferences) -> Choice {
    match Text::from_bytes(font) {
        Some(text) => show_with_text(summary, text, preferences, None, |_| {}),
        None => {
            tracing::warn!("crash window unavailable: the UI font does not parse");
            Choice::Unavailable
        }
    }
}

/// Runs one crash window, retaining upload workers until its actions finish.
fn show_with_text(
    summary: CrashSummary,
    text: Text,
    preferences: Preferences,
    layout: Option<InstallLayout>,
    selected: impl FnOnce(Choice),
) -> Choice {
    let Some(faces) = bootstrap_ui::built_in_faces() else {
        selected(Choice::Unavailable);
        return Choice::Unavailable;
    };
    let Ok(event_loop) = EventLoop::new() else {
        selected(Choice::Unavailable);
        return Choice::Unavailable;
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let logo = bootstrap_ui::decode_logo();
    let mut app = app::CrashApp::new(summary, text, faces, logo, preferences, layout);
    let failed = event_loop.run_app(&mut app).is_err();
    let choice = app.choice().unwrap_or(if failed {
        Choice::Unavailable
    } else {
        Choice::Close
    });
    let upload = app.take_upload();
    finish_choice(app, choice, upload, selected)
}

/// Destroys the window owner before applying the selected action and waiting for its upload.
fn finish_choice<T>(
    window_owner: T,
    choice: Choice,
    upload: Option<std::thread::JoinHandle<()>>,
    selected: impl FnOnce(Choice),
) -> Choice {
    drop(window_owner);
    selected(choice);
    if let Some(worker) = upload {
        let _ = worker.join();
    }
    choice
}

#[cfg(test)]
mod tests;
