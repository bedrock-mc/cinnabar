//! The crash report window: what happened, where the report is, and what to do next. It draws
//! with the setup window's panorama, panel, type and controls, so it needs no game assets.

mod app;
mod desktop;
mod discord;
mod view;

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use winit::{
    dpi::LogicalSize,
    event_loop::{ControlFlow, EventLoop},
};

use super::{EXIT_QUIT, EXIT_UNAVAILABLE, Preferences, canvas::Text, input::Actions};
use launcher::{PRODUCT_NAME, install_layout::InstallLayout};

/// The crash window's default logical size.
const CRASH_SIZE: LogicalSize<f64> = LogicalSize::new(960.0, 640.0);
/// How long Copy report shows its outcome before reading "Copy report" again.
const COPY_FEEDBACK: Duration = Duration::from_secs(2);
/// The exit code when the player chose Restart.
pub const EXIT_RESTART: i32 = 0;

/// What one crash report says, as the crash window shows it.
#[derive(Clone, Debug)]
pub struct CrashSummary {
    /// What the player was doing, completing "while you were …".
    pub activity: String,
    /// The fault, such as an exception code and address.
    pub fault: String,
    /// The faulting module offset and thread.
    pub location: String,
    pub version: String,
    pub graphics: String,
    pub session: String,
    /// The client log's last lines, oldest first.
    pub log_tail: Vec<String>,
    /// The last packets before the crash, oldest first.
    pub last_packets: Vec<String>,
    /// The report folder on this computer.
    pub folder: PathBuf,
}

impl CrashSummary {
    fn title() -> String {
        format!("{PRODUCT_NAME} crashed")
    }

    fn body(&self) -> String {
        format!(
            "{PRODUCT_NAME} stopped unexpectedly while you were {}. A crash report was saved on \
             this computer; nothing was sent anywhere.",
            self.activity
        )
    }

    /// The summary card's rows; the issue text uses the same labels.
    fn facts(&self) -> [(&'static str, &str); 5] {
        [
            ("What happened", &self.fault),
            ("Where", &self.location),
            ("Version", &self.version),
            ("Graphics", &self.graphics),
            ("Session", &self.session),
        ]
    }

    fn log_heading(&self) -> String {
        format!("client.log, last {} lines", self.log_tail.len())
    }

    fn packets_heading(&self) -> String {
        format!("Last {} packets, newest last", self.last_packets.len())
    }

    /// The details view's lines: the last packets, then the log tail, so the view (which opens
    /// at its end) shows the log's final lines first.
    fn detail_lines(&self) -> Vec<DetailLine> {
        let mut lines = vec![DetailLine::new(Tone::Heading, self.packets_heading())];
        lines.extend(
            self.last_packets
                .iter()
                .map(|line| DetailLine::new(Tone::Plain, line.clone())),
        );
        lines.push(DetailLine::new(Tone::Plain, String::new()));
        lines.push(DetailLine::new(Tone::Heading, self.log_heading()));
        lines.extend(
            self.log_tail
                .iter()
                .map(|line| DetailLine::new(Tone::of(line), line.clone())),
        );
        lines
    }

    /// A Markdown issue body for Copy report. It leaves out the folder path, which names the
    /// player's account on most systems.
    fn issue_markdown(&self) -> String {
        let mut out = format!("### {}\n\n", Self::title());
        out.push_str(&format!("While {}.\n\n| | |\n|---|---|\n", self.activity));
        for (label, value) in self.facts() {
            out.push_str(&format!("| {label} | `{value}` |\n"));
        }
        for (heading, lines) in [
            (self.log_heading(), &self.log_tail),
            (self.packets_heading(), &self.last_packets),
        ] {
            out.push_str(&format!(
                "\n<details><summary>{heading}</summary>\n\n```text\n"
            ));
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
            out.push_str("```\n\n</details>\n");
        }
        out.push_str("\nAttach `report.json` from the crash report folder if you can.\n");
        out
    }
}

/// How a details line is coloured.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tone {
    Heading,
    Plain,
    Warning,
    Error,
}

impl Tone {
    fn of(line: &str) -> Self {
        if line.contains(" ERROR ") || line.contains("panicked") {
            Self::Error
        } else if line.contains(" WARN ") {
            Self::Warning
        } else {
            Self::Plain
        }
    }
}

#[derive(Clone, Debug)]
struct DetailLine {
    tone: Tone,
    text: String,
}

impl DetailLine {
    fn new(tone: Tone, text: String) -> Self {
        Self { tone, text }
    }
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

/// Shows the crash window for `summary` with the UI font `font` (Cinnangles Sans) and returns
/// [`EXIT_RESTART`] when the player chose Restart, 3 for Close and 4 when no window could open.
pub fn run_crash_preview(summary: CrashSummary, font: &[u8]) -> i32 {
    let Some(text) = Text::from_bytes(font) else {
        eprintln!("crash window unavailable: unusable UI font");
        return EXIT_UNAVAILABLE;
    };
    let Some(faces) = super::built_in_faces() else {
        return EXIT_UNAVAILABLE;
    };
    let Ok(event_loop) = EventLoop::new() else {
        return EXIT_UNAVAILABLE;
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let preferences = InstallLayout::discover()
        .map(|layout| Preferences::load(&layout))
        .unwrap_or_default();
    let mut app = app::CrashApp::new(summary, text, faces, super::decode_logo(), preferences);
    let failed = event_loop.run_app(&mut app).is_err();
    app.exit_code()
        .unwrap_or(if failed { EXIT_UNAVAILABLE } else { EXIT_QUIT })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> CrashSummary {
        CrashSummary {
            activity: "joining a server".into(),
            fault: "EXCEPTION_ACCESS_VIOLATION reading 0x18".into(),
            location: "cinnabar.exe+0x1a2b3c on thread world-2".into(),
            version: "0.1.20".into(),
            graphics: "Vulkan".into(),
            session: "32 min".into(),
            log_tail: vec![
                "12:00:01 INFO  join started".into(),
                "12:00:02 WARN  slow chunk".into(),
                "12:00:03 ERROR render failed".into(),
            ],
            last_packets: vec!["in  LevelChunk 4 KiB".into()],
            folder: PathBuf::from("/home/someone/crash-reports/one"),
        }
    }

    #[test]
    fn issue_text_carries_every_fact_and_both_logs_but_not_the_folder() {
        let summary = summary();
        let markdown = summary.issue_markdown();
        for (label, value) in summary.facts() {
            assert!(markdown.contains(&format!("| {label} | `{value}` |")));
        }
        for line in summary.log_tail.iter().chain(&summary.last_packets) {
            assert!(markdown.contains(line.as_str()));
        }
        assert!(!markdown.contains("someone"));
    }

    #[test]
    fn details_colour_errors_and_warnings() {
        let tones: Vec<_> = summary().detail_lines().iter().map(|l| l.tone).collect();
        assert_eq!(
            tones,
            [
                Tone::Heading,
                Tone::Plain,
                Tone::Plain,
                Tone::Heading,
                Tone::Plain,
                Tone::Warning,
                Tone::Error
            ]
        );
    }

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
