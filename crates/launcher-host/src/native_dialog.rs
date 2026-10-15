//! Native consent and status dialogs for the pre-window lifecycle flows: OS APIs where they exist,
//! then each OS's stock tooling.

use std::{
    io::{BufRead, IsTerminal, Write},
    process::{Command, Stdio},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Consent {
    Accepted,
    Declined,
    /// No dialog, tool or terminal could ask.
    Unavailable,
}

impl Consent {
    const fn from_answer(accepted: bool) -> Self {
        if accepted {
            Self::Accepted
        } else {
            Self::Declined
        }
    }
}

pub trait Prompter {
    /// Asks a yes/no question.
    fn confirm(&self, title: &str, body: &str) -> Consent;
    /// Best-effort, non-blocking progress message.
    fn info(&self, title: &str, body: &str);
    /// Blocking error message.
    fn alert(&self, title: &str, body: &str);
}

pub struct NativePrompter;

#[derive(Clone, Copy)]
enum Kind {
    Confirm,
    Info,
    Alert,
}

impl Prompter for NativePrompter {
    fn confirm(&self, title: &str, body: &str) -> Consent {
        #[cfg(windows)]
        let native = {
            use crate::desktop::windows::{MessageKind, message_box};
            message_box(MessageKind::Confirm, title, body)
        };
        #[cfg(not(windows))]
        let native = None;
        confirm_with(native, commands(Kind::Confirm, title, body), || {
            tty_confirm(title, body)
        })
    }

    #[expect(
        clippy::print_stderr,
        reason = "the terminal is the user-facing fallback when no notification or dialog surface exists"
    )]
    fn info(&self, title: &str, body: &str) {
        let surfaces = commands(Kind::Info, title, body);
        let title = title.to_owned();
        let body = body.to_owned();
        std::thread::spawn(move || {
            if !notify(&title, &body) && !show_message(surfaces) {
                eprintln!("{title}: {body}");
            }
        });
    }

    #[expect(
        clippy::print_stderr,
        reason = "the terminal is the user-facing fallback when no dialog or notification surface exists"
    )]
    fn alert(&self, title: &str, body: &str) {
        #[cfg(windows)]
        {
            use crate::desktop::windows::{MessageKind, message_box};
            if message_box(MessageKind::Alert, title, body).is_some() {
                return;
            }
        }
        if show_message(commands(Kind::Alert, title, body)) || notify(title, body) {
            return;
        }
        eprintln!("{title}: {body}");
    }
}

/// The native answer, else the first tool that launches, else `fallback`.
fn confirm_with(
    native: Option<bool>,
    commands: Vec<Vec<String>>,
    fallback: impl FnOnce() -> Consent,
) -> Consent {
    native
        .or_else(|| commands.iter().find_map(|argv| run(argv)))
        .map_or_else(fallback, Consent::from_answer)
}

/// Posts a desktop notification without an outside tool where the OS has a service for it.
fn notify(title: &str, body: &str) -> bool {
    #[cfg(target_os = "linux")]
    return crate::desktop::dbus::notify(title, body).is_ok();
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (title, body);
        false
    }
}

/// Tries message tools in order, requiring successful delivery before stopping.
fn show_message(commands: Vec<Vec<String>>) -> bool {
    commands.iter().any(|argv| run(argv) == Some(true))
}

/// Waits for a tracked tool; `None` means it could not be launched.
fn run(argv: &[String]) -> Option<bool> {
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = crate::lifecycle::children::spawn(&mut command).ok()?;
    Some(child.wait().is_some_and(|status| status.success()))
}

/// What the player chose in the crash dialog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CrashChoice {
    Restart,
    OpenFolder,
    CopyReport,
    Close,
    /// No dialog or tool could ask.
    Unavailable,
}

/// Asks what to do about a crash with the crash window's actions, for when that window cannot
/// open; blocks until the player chooses. macOS dialogs hold three buttons, so they offer no
/// Copy report.
pub fn crash_choice(title: &str, body: &str) -> CrashChoice {
    #[cfg(windows)]
    {
        crate::desktop::windows::crash_dialog::crash_dialog(title, body)
    }
    #[cfg(not(windows))]
    {
        crash_tools(title, body)
            .into_iter()
            .find_map(|(tool, argv)| run_answer(&argv).map(|answer| tool.choice(&answer)))
            .unwrap_or(CrashChoice::Unavailable)
    }
}

/// A dialog tool's exit code and printed output.
#[cfg(any(not(windows), test))]
struct Answer {
    code: Option<i32>,
    output: String,
}

#[cfg(any(not(windows), test))]
#[derive(Clone, Copy, Debug)]
enum CrashTool {
    Osascript,
    Zenity,
    Kdialog,
}

#[cfg(any(not(windows), test))]
impl CrashTool {
    /// Reads the tool's answer; anything unrecognized, such as closing the dialog, is Close.
    fn choice(self, answer: &Answer) -> CrashChoice {
        let output = answer.output.trim();
        match self {
            Self::Osascript => match output.strip_prefix("button returned:") {
                Some("Restart") => CrashChoice::Restart,
                Some("Open folder") => CrashChoice::OpenFolder,
                _ => CrashChoice::Close,
            },
            Self::Zenity => match (answer.code, output) {
                (Some(0), _) => CrashChoice::Restart,
                (_, "Open folder") => CrashChoice::OpenFolder,
                (_, "Copy report") => CrashChoice::CopyReport,
                _ => CrashChoice::Close,
            },
            Self::Kdialog => match answer.code {
                Some(0) => CrashChoice::Restart,
                Some(1) => CrashChoice::OpenFolder,
                _ => CrashChoice::Close,
            },
        }
    }
}

#[cfg(not(windows))]
fn crash_tools(title: &str, body: &str) -> Vec<(CrashTool, Vec<String>)> {
    if cfg!(target_os = "macos") {
        let script = format!(
            "display dialog {} with title {} buttons {{\"Close\", \"Open folder\", \"Restart\"}} default button \"Restart\" with icon stop",
            applescript_quote(body),
            applescript_quote(title)
        );
        return vec![(
            CrashTool::Osascript,
            vec!["osascript".into(), "-e".into(), script],
        )];
    }
    vec![
        (
            CrashTool::Zenity,
            vec![
                "zenity".into(),
                "--question".into(),
                "--no-markup".into(),
                "--width=560".into(),
                "--icon-name=dialog-error".into(),
                "--ok-label=Restart".into(),
                "--cancel-label=Close".into(),
                "--extra-button=Open folder".into(),
                "--extra-button=Copy report".into(),
                format!("--title={title}"),
                format!("--text={body}"),
            ],
        ),
        (
            CrashTool::Kdialog,
            vec![
                "kdialog".into(),
                "--title".into(),
                title.into(),
                "--yes-label".into(),
                "Restart".into(),
                "--no-label".into(),
                "Open folder".into(),
                "--cancel-label".into(),
                "Close".into(),
                "--warningyesnocancel".into(),
                body.into(),
            ],
        ),
    ]
}

/// Waits for a tracked dialog tool and reads its answer; `None` means it could not launch.
#[cfg(not(windows))]
fn run_answer(argv: &[String]) -> Option<Answer> {
    use std::io::Read;

    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = crate::lifecycle::children::spawn(&mut command).ok()?;
    let mut output = String::new();
    if let Some(mut stdout) = child.take_stdout() {
        let _ = stdout.read_to_string(&mut output);
    }
    let status = child.wait()?;
    Some(Answer {
        code: status.code(),
        output,
    })
}

#[expect(
    clippy::print_stderr,
    reason = "the consent prompt is written to the terminal the user answers on"
)]
fn tty_confirm(title: &str, body: &str) -> Consent {
    if !std::io::stdin().is_terminal() {
        return Consent::Unavailable;
    }
    eprint!("{title}\n{body}\nType `yes` to continue: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    Consent::from_answer(
        std::io::stdin().lock().read_line(&mut line).is_ok()
            && line.trim().eq_ignore_ascii_case("yes"),
    )
}

fn commands(kind: Kind, title: &str, body: &str) -> Vec<Vec<String>> {
    let owned = |parts: &[&str]| {
        parts
            .iter()
            .map(|part| (*part).to_owned())
            .collect::<Vec<_>>()
    };
    if cfg!(target_os = "macos") {
        let (t, b) = (applescript_quote(title), applescript_quote(body));
        let script = match kind {
            Kind::Confirm => format!(
                "display dialog {b} with title {t} buttons {{\"Quit\", \"Continue\"}} default button \"Continue\" cancel button \"Quit\""
            ),
            Kind::Info => format!("display notification {b} with title {t}"),
            Kind::Alert => format!(
                "display dialog {b} with title {t} buttons {{\"OK\"}} default button \"OK\" with icon stop"
            ),
        };
        return vec![vec!["osascript".into(), "-e".into(), script]];
    }
    // Windows uses message boxes directly and has no stock notification tool.
    if cfg!(target_os = "windows") {
        return Vec::new();
    }
    match kind {
        Kind::Confirm => vec![
            vec![
                "zenity".into(),
                "--question".into(),
                "--width=520".into(),
                "--ok-label=Continue".into(),
                "--cancel-label=Quit".into(),
                format!("--title={title}"),
                format!("--text={body}"),
            ],
            vec![
                "kdialog".into(),
                "--title".into(),
                title.into(),
                "--yesno".into(),
                body.into(),
            ],
        ],
        Kind::Alert => vec![
            vec![
                "zenity".into(),
                "--error".into(),
                format!("--title={title}"),
                format!("--text={body}"),
            ],
            vec![
                "kdialog".into(),
                "--title".into(),
                title.into(),
                "--error".into(),
                body.into(),
            ],
        ],
        Kind::Info => vec![owned(&["notify-send", title, body])],
    }
}

fn applescript_quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn review_failed_message_tool_allows_the_next_fallback() {
        let path =
            std::env::temp_dir().join(format!("cinnabar-dialog-fallback-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let first = vec!["sh".into(), "-c".into(), "exit 1".into()];
        let second = vec![
            "sh".into(),
            "-c".into(),
            "touch \"$1\"".into(),
            "test".into(),
            path.to_string_lossy().into_owned(),
        ];
        assert!(show_message(vec![first, second]));
        let delivered = path.exists();
        let _ = std::fs::remove_file(path);
        assert!(delivered);
    }

    #[test]
    fn applescript_quoting_escapes_quotes_backslashes_and_newlines() {
        assert_eq!(applescript_quote("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
    }

    #[test]
    fn crash_dialog_answers_map_to_the_window_actions() {
        let answer = |code: i32, output: &str| Answer {
            code: Some(code),
            output: output.to_owned(),
        };
        for (tool, answer, choice) in [
            (
                CrashTool::Osascript,
                answer(0, "button returned:Restart\n"),
                CrashChoice::Restart,
            ),
            (
                CrashTool::Osascript,
                answer(0, "button returned:Open folder\n"),
                CrashChoice::OpenFolder,
            ),
            (
                CrashTool::Osascript,
                answer(0, "button returned:Close\n"),
                CrashChoice::Close,
            ),
            (CrashTool::Zenity, answer(0, ""), CrashChoice::Restart),
            (
                CrashTool::Zenity,
                answer(1, "Open folder\n"),
                CrashChoice::OpenFolder,
            ),
            (
                CrashTool::Zenity,
                answer(1, "Copy report\n"),
                CrashChoice::CopyReport,
            ),
            (CrashTool::Zenity, answer(1, ""), CrashChoice::Close),
            (CrashTool::Kdialog, answer(0, ""), CrashChoice::Restart),
            (CrashTool::Kdialog, answer(1, ""), CrashChoice::OpenFolder),
            (CrashTool::Kdialog, answer(2, ""), CrashChoice::Close),
        ] {
            assert_eq!(tool.choice(&answer), choice, "{tool:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn confirm_takes_the_first_launched_answer_and_reports_no_surface() {
        let sh = |code: &str| vec!["sh".into(), "-c".into(), format!("exit {code}")];
        let missing = vec!["cinnabar-no-such-dialog".to_owned()];
        let unavailable = || Consent::Unavailable;
        assert_eq!(
            confirm_with(None, vec![missing.clone(), sh("0")], unavailable),
            Consent::Accepted
        );
        assert_eq!(
            confirm_with(None, vec![sh("1"), sh("0")], unavailable),
            Consent::Declined
        );
        assert_eq!(
            confirm_with(Some(true), vec![sh("1")], unavailable),
            Consent::Accepted
        );
        assert_eq!(
            confirm_with(None, vec![missing], unavailable),
            Consent::Unavailable
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn every_kind_has_a_command_on_the_supported_platforms() {
        for kind in [Kind::Confirm, Kind::Alert] {
            assert!(!commands(kind, "t", "b").is_empty());
        }
    }
}
