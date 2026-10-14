//! What the crash window says about one report: its title and sentence, the summary rows, the
//! details log and the Markdown that Copy report puts on the clipboard.

use launcher::PRODUCT_NAME;

use super::{CrashSummary, Headline};

/// The summary row and details heading for the fault.
const FAULT_LABEL: &str = "What happened";

impl CrashSummary {
    /// The window title line, such as "Cinnabar crashed".
    pub(crate) fn title(&self) -> String {
        match self.headline {
            Headline::Crashed { .. } => format!("{PRODUCT_NAME} crashed"),
            Headline::CouldNotStart => format!("{PRODUCT_NAME} couldn't start"),
        }
    }

    /// The opening sentence under the title: what happened and that nothing was sent.
    pub(crate) fn body(&self) -> String {
        match &self.headline {
            Headline::Crashed { activity } => format!(
                "{PRODUCT_NAME} stopped unexpectedly while you were {activity}. A crash report was \
                 saved on this computer; nothing was sent anywhere."
            ),
            Headline::CouldNotStart => format!(
                "{PRODUCT_NAME} couldn't start. The error is below and a report was saved on this \
                 computer; nothing was sent anywhere."
            ),
        }
    }

    /// The summary card's rows; the issue text uses the same labels. The fault row holds only
    /// the fault's first line.
    pub(super) fn facts(&self) -> [(&'static str, &str); 5] {
        [
            (FAULT_LABEL, self.fault_line()),
            ("Where", &self.location),
            ("Version", &self.version),
            ("Graphics", &self.graphics),
            ("Session", &self.session),
        ]
    }

    /// The fault's first line; [`Self::full_fault`] carries the rest into the details.
    fn fault_line(&self) -> &str {
        self.fault.lines().next().unwrap_or_default()
    }

    /// The whole fault when its summary row cannot carry it: every start failure, and any
    /// multi-line fault.
    fn full_fault(&self) -> Option<&str> {
        (self.headline == Headline::CouldNotStart || self.fault.lines().nth(1).is_some())
            .then_some(self.fault.as_str())
    }

    fn log_heading(&self) -> String {
        format!("client.log, last {} lines", self.log_tail.len())
    }

    fn packets_heading(&self) -> String {
        format!("Last {} packets, newest last", self.last_packets.len())
    }

    /// The details view's lines: the last packets (when there are any), the log tail, then the
    /// whole fault when its row is cut short. The view opens at its end, so it shows the fault,
    /// or else the log's final lines, first.
    pub(super) fn detail_lines(&self) -> Vec<DetailLine> {
        let mut lines = Vec::new();
        if !self.last_packets.is_empty() {
            lines.push(DetailLine::new(Tone::Heading, self.packets_heading()));
            lines.extend(
                self.last_packets
                    .iter()
                    .map(|line| DetailLine::new(Tone::Plain, line.clone())),
            );
            lines.push(DetailLine::new(Tone::Plain, String::new()));
        }
        lines.push(DetailLine::new(Tone::Heading, self.log_heading()));
        lines.extend(
            self.log_tail
                .iter()
                .map(|line| DetailLine::new(Tone::of(line), line.clone())),
        );
        if let Some(fault) = self.full_fault() {
            lines.push(DetailLine::new(Tone::Plain, String::new()));
            lines.push(DetailLine::new(Tone::Heading, FAULT_LABEL.to_owned()));
            lines.extend(
                fault
                    .lines()
                    .map(|line| DetailLine::new(Tone::Error, line.to_owned())),
            );
        }
        lines
    }

    /// A Markdown issue body for Copy report. It leaves out the folder path, which names the
    /// player's account on most systems, and writes the player's home directory as `~`.
    pub(super) fn issue_markdown(&self) -> String {
        let markdown = self.markdown();
        match home() {
            Some(home) => redact_home(&markdown, &home, cfg!(windows)),
            None => markdown,
        }
    }

    fn markdown(&self) -> String {
        let mut out = format!("### {}\n\n", self.title());
        if let Headline::Crashed { activity } = &self.headline {
            out.push_str(&format!("While {activity}.\n\n"));
        }
        out.push_str("| | |\n|---|---|\n");
        for (label, value) in self.facts() {
            out.push_str(&format!("| {label} | `{value}` |\n"));
        }
        let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
        if let Some(fault) = self.full_fault() {
            sections.push((FAULT_LABEL.to_owned(), fault.lines().collect()));
        }
        sections.push((
            self.log_heading(),
            self.log_tail.iter().map(String::as_str).collect(),
        ));
        if !self.last_packets.is_empty() {
            sections.push((
                self.packets_heading(),
                self.last_packets.iter().map(String::as_str).collect(),
            ));
        }
        for (heading, lines) in sections {
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

/// The player's home directory, which Copy report never includes.
fn home() -> Option<String> {
    std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok()
        .filter(|home| !home.is_empty())
}

/// Replaces every `home` path in `text` with `~`. Either separator matches either separator, case
/// is ignored when `ignore_case` (Windows paths), and a match must end where a path component
/// does, so a sibling such as `/home/ann2` keeps its name.
fn redact_home(text: &str, home: &str, ignore_case: bool) -> String {
    let home: Vec<char> = home.trim_end_matches(['/', '\\']).chars().collect();
    if home.is_empty() {
        return text.to_owned();
    }
    let same = |a: char, b: char| match (a, b) {
        ('/' | '\\', '/' | '\\') => true,
        _ if ignore_case => a.to_lowercase().eq(b.to_lowercase()),
        _ => a == b,
    };
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < chars.len() {
        let end = at + home.len();
        let found = chars
            .get(at..end)
            .is_some_and(|window| window.iter().zip(&home).all(|(a, b)| same(*a, *b)))
            && chars
                .get(end)
                .is_none_or(|next| !(next.is_alphanumeric() || matches!(next, '_' | '-')));
        if found {
            out.push('~');
            at = end;
        } else {
            out.push(chars[at]);
            at += 1;
        }
    }
    out
}

/// How a details line is coloured.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Tone {
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
pub(super) struct DetailLine {
    pub tone: Tone,
    pub text: String,
}

impl DetailLine {
    fn new(tone: Tone, text: String) -> Self {
        Self { tone, text }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn summary() -> CrashSummary {
        CrashSummary {
            headline: Headline::Crashed {
                activity: "joining a server".into(),
            },
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

    fn could_not_start() -> CrashSummary {
        CrashSummary {
            headline: Headline::CouldNotStart,
            fault: "missing carrier hud.bin\nrun `make assets` to rebuild it".into(),
            last_packets: Vec::new(),
            ..summary()
        }
    }

    #[test]
    fn issue_text_carries_every_fact_and_both_logs_but_not_the_folder() {
        let summary = summary();
        let markdown = summary.issue_markdown();
        assert!(markdown.starts_with(&format!("### {PRODUCT_NAME} crashed\n")));
        assert!(markdown.contains("While joining a server."));
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
    fn without_packets_neither_details_nor_issue_text_mention_them() {
        let summary = CrashSummary {
            last_packets: Vec::new(),
            ..summary()
        };
        let packets = summary.packets_heading();
        assert!(
            summary
                .detail_lines()
                .iter()
                .all(|line| line.text != packets)
        );
        assert_eq!(summary.detail_lines()[0].text, summary.log_heading());
        let markdown = summary.issue_markdown();
        assert!(!markdown.contains("packets"));
        assert!(markdown.contains(&summary.log_heading()));
    }

    #[test]
    fn a_start_failure_names_itself_and_shows_the_whole_error() {
        let summary = could_not_start();
        assert_eq!(summary.title(), format!("{PRODUCT_NAME} couldn't start"));
        assert!(summary.body().contains("couldn't start"));
        assert_eq!(summary.facts()[0], (FAULT_LABEL, "missing carrier hud.bin"));
        let details: Vec<_> = summary
            .detail_lines()
            .into_iter()
            .map(|line| line.text)
            .collect();
        assert!(details.ends_with(&[
            FAULT_LABEL.to_owned(),
            "missing carrier hud.bin".to_owned(),
            "run `make assets` to rebuild it".to_owned(),
        ]));
        let markdown = summary.issue_markdown();
        assert!(markdown.starts_with(&format!("### {PRODUCT_NAME} couldn't start\n")));
        assert!(!markdown.contains("While "));
        assert!(markdown.contains("| What happened | `missing carrier hud.bin` |"));
        assert!(markdown.contains("run `make assets` to rebuild it\n"));
    }

    #[test]
    fn a_single_line_crash_fault_stays_in_its_row() {
        let summary = summary();
        assert!(summary.full_fault().is_none());
        assert!(
            !summary
                .markdown()
                .contains(&format!("<summary>{FAULT_LABEL}"))
        );
    }

    #[test]
    fn home_directories_become_a_tilde_with_either_separator() {
        let text =
            "logs at C:\\Users\\Ann\\AppData\\logs\nsaved c:/users/ann/crash\nC:\\Users\\Ann";
        assert_eq!(
            redact_home(text, "C:\\Users\\Ann", true),
            "logs at ~\\AppData\\logs\nsaved ~/crash\n~"
        );
        assert_eq!(
            redact_home("/home/ann/x /HOME/ANN/y /home/ann2/z", "/home/ann/", false),
            "~/x /HOME/ANN/y /home/ann2/z"
        );
        assert_eq!(redact_home("/a/b", "/", false), "/a/b");
    }

    #[test]
    fn copy_report_never_contains_the_home_directory() {
        let Some(home) = home() else {
            eprintln!("skipping: no home directory variable is set");
            return;
        };
        let summary = CrashSummary {
            log_tail: vec![format!("12:00:01 INFO  logs at {home}/logs")],
            ..could_not_start()
        };
        let markdown = summary.issue_markdown();
        assert!(!markdown.contains(&home), "{markdown}");
        assert!(markdown.contains("logs at ~/logs"));
    }
}
