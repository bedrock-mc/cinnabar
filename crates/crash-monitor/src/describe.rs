//! A report as the crash window shows it.

use std::path::PathBuf;

use crash_report::{
    folder::Logs,
    report::{Kind, Report},
};
use launcher_host::lifecycle::crash::state::{
    DIMENSION, Dimension, SERVER_KIND, SERVER_PACKS, SESSION_PHASE, ServerKind, SessionPhase,
};

use crate::{
    outcome,
    window::{CrashSummary, Headline},
};

/// Client log lines the details view lists, newest last.
const LOG_TAIL_LINES: usize = 200;

/// Summarizes `report`, saved with `logs` in `folder`.
pub(crate) fn summary(report: &Report, logs: &Logs, folder: PathBuf) -> CrashSummary {
    let (fault, location) = fault_and_location(report);
    CrashSummary {
        headline: match report.kind {
            Kind::Startup => Headline::CouldNotStart,
            Kind::Runtime => Headline::Stopped {
                activity: activity(report),
            },
            _ => Headline::Crashed {
                activity: activity(report),
            },
        },
        fault,
        location,
        version: format!(
            "{} ({}, {})",
            report.release,
            short_commit(&report.commit),
            report.build_profile
        ),
        graphics: graphics(report),
        session: session(report),
        log_tail: log_tail(&logs.client),
        last_packets: Vec::new(),
        folder,
    }
}

/// Completes "while you were …".
fn activity(report: &Report) -> String {
    let kind = report
        .game_state
        .get(SERVER_KIND)
        .map(String::as_str)
        .and_then(ServerKind::from_name);
    let phase = report
        .game_state
        .get(SESSION_PHASE)
        .map(String::as_str)
        .and_then(SessionPhase::from_name);
    match phase {
        Some(SessionPhase::Menus) => "in the menus".to_owned(),
        Some(SessionPhase::Joining) => format!("joining {}", destination(kind)),
        Some(SessionPhase::InWorld) => format!("playing {}", place(kind)),
        None => "starting the game".to_owned(),
    }
}

fn destination(kind: Option<ServerKind>) -> &'static str {
    match kind {
        Some(ServerKind::Server) => "a server",
        Some(ServerKind::FeaturedServer) => "a featured server",
        Some(ServerKind::Realm) => "a Realm",
        Some(ServerKind::FriendWorld) => "a friend's world",
        Some(ServerKind::Experience) => "an experience",
        Some(ServerKind::LocalWorld) => "a local world",
        None => "a world",
    }
}

fn place(kind: Option<ServerKind>) -> &'static str {
    match kind {
        Some(ServerKind::Server) => "on a server",
        Some(ServerKind::FeaturedServer) => "on a featured server",
        Some(ServerKind::Realm) => "on a Realm",
        Some(ServerKind::FriendWorld) => "in a friend's world",
        Some(ServerKind::Experience) => "in an experience",
        Some(ServerKind::LocalWorld) => "in a local world",
        None => "in a world",
    }
}

/// The "What happened" and "Where" rows.
fn fault_and_location(report: &Report) -> (String, String) {
    let thread = report
        .thread
        .name
        .clone()
        .unwrap_or_else(|| report.thread.id.clone());
    match report.kind {
        Kind::Panic => {
            // `panicked at <file:line:column>:\n<message>`
            let (head, message) = report
                .message
                .split_once('\n')
                .unwrap_or((report.message.as_str(), ""));
            let at = head
                .strip_prefix("panicked at ")
                .map_or(head, |at| at.trim_end_matches(':'));
            let message = if message.is_empty() { head } else { message };
            (
                format!("Panic: {}", message.trim()),
                format!("{at} on thread {thread}"),
            )
        }
        Kind::Native => {
            let address = report
                .native
                .as_ref()
                .and_then(|native| native.get("address"))
                .and_then(|address| address.as_str())
                .map(|address| format!(" at {address}"))
                .unwrap_or_default();
            let location = outcome::native_location(report).map_or_else(
                || format!("thread {thread}"),
                |at| format!("{at} on thread {thread}"),
            );
            (format!("{}{address}", report.message), location)
        }
        Kind::Startup => (report.message.clone(), "Before the game started".to_owned()),
        Kind::Runtime => (
            report.message.clone(),
            "While the game was running".to_owned(),
        ),
        _ => (report.message.clone(), "Unknown: no crash dump".to_owned()),
    }
}

fn short_commit(commit: &str) -> &str {
    commit.get(..9).unwrap_or(commit)
}

fn graphics(report: &Report) -> String {
    let backend = report
        .renderer
        .as_ref()
        .and_then(|renderer| renderer.get("backend"))
        .and_then(|backend| backend.as_str());
    let parts: Vec<String> = [
        report.gpu.clone(),
        report
            .driver
            .as_ref()
            .map(|driver| format!("driver {driver}")),
        backend.map(backend_name),
    ]
    .into_iter()
    .flatten()
    .collect();
    if parts.is_empty() {
        "Not started yet".to_owned()
    } else {
        parts.join(", ")
    }
}

fn backend_name(backend: &str) -> String {
    match backend {
        "vulkan" => "Vulkan".to_owned(),
        "dx12" => "DirectX 12".to_owned(),
        "metal" => "Metal".to_owned(),
        "gl" => "OpenGL".to_owned(),
        other => other.to_owned(),
    }
}

/// Uptime, then the dimension and server packs while in a world.
fn session(report: &Report) -> String {
    let mut session = uptime(report.uptime_ms);
    if let Some(dimension) = report.game_state.get(DIMENSION) {
        match Dimension::from_name(dimension) {
            Some(Dimension::Overworld) => session.push_str(", in the Overworld"),
            Some(Dimension::Nether) => session.push_str(", in the Nether"),
            Some(Dimension::TheEnd) => session.push_str(", in the End"),
            None => session.push_str(&format!(", in dimension {dimension}")),
        }
    }
    match report.game_state.get(SERVER_PACKS).map(String::as_str) {
        None | Some("0") => {}
        Some("1") => session.push_str(", 1 server pack"),
        Some(count) => session.push_str(&format!(", {count} server packs")),
    }
    session
}

fn uptime(ms: u64) -> String {
    let seconds = ms / 1000;
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        _ => format!("{} h {} min", seconds / 3600, seconds % 3600 / 60),
    }
}

/// The client log section's last lines.
fn log_tail(client: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(client);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(LOG_TAIL_LINES)..]
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crash_report::report::{Identity, Thread};

    fn report(kind: Kind, message: &str) -> Report {
        let identity = Identity {
            session_id: "0123456789abcdef".into(),
            started_at_ms: 0,
            release: "0.1.20".into(),
            commit: "4ec3296c5aaaa".into(),
            build_profile: "release".into(),
        };
        let mut report = Report::new(kind, message.into(), &identity, 1_920_000);
        report.thread = Thread {
            name: Some("world-2".into()),
            id: "7".into(),
        };
        report
    }

    #[test]
    fn panics_split_their_location_from_their_message() {
        let report = report(
            Kind::Panic,
            "panicked at src/world.rs:12:5:\nindex out of range",
        );
        let summary = summary(&report, &Logs::default(), PathBuf::new());
        assert_eq!(summary.fault, "Panic: index out of range");
        assert_eq!(summary.location, "src/world.rs:12:5 on thread world-2");
        assert_eq!(summary.version, "0.1.20 (4ec3296c5, release)");
        assert_eq!(summary.session, "32 min");
        assert_eq!(summary.graphics, "Not started yet");
    }

    #[test]
    fn native_faults_name_their_module_offset() {
        let mut report = report(Kind::Native, "EXCEPTION_ACCESS_VIOLATION_READ");
        report.native = Some(serde_json::json!({
            "reason": "EXCEPTION_ACCESS_VIOLATION_READ",
            "address": "0x18",
            "fault": {"address": "0x7FF61A2B3C", "module": "cinnabar.exe", "offset": "0x1A2B3C"},
        }));
        report.gpu = Some("GPU".into());
        report.driver = Some("1.2".into());
        report.renderer = Some(serde_json::json!({"backend": "vulkan"}));
        let summary = summary(&report, &Logs::default(), PathBuf::new());
        assert_eq!(summary.fault, "EXCEPTION_ACCESS_VIOLATION_READ at 0x18");
        assert_eq!(summary.location, "cinnabar.exe+0x1A2B3C on thread world-2");
        assert_eq!(summary.graphics, "GPU, driver 1.2, Vulkan");
    }

    #[test]
    fn activity_follows_the_session_phase_and_server_kind() {
        let mut report = report(Kind::Native, "fault");
        assert!(matches!(
            summary(&report, &Logs::default(), PathBuf::new()).headline,
            Headline::Crashed { ref activity } if activity == "starting the game"
        ));
        for (phase, kind, expected) in [
            (SessionPhase::Menus, None, "in the menus"),
            (
                SessionPhase::Joining,
                Some(ServerKind::Realm),
                "joining a Realm",
            ),
            (
                SessionPhase::InWorld,
                Some(ServerKind::LocalWorld),
                "playing in a local world",
            ),
            (SessionPhase::InWorld, None, "playing in a world"),
        ] {
            report.game_state.insert(SESSION_PHASE.into(), phase.name());
            match kind {
                Some(kind) => report.game_state.insert(SERVER_KIND.into(), kind.name()),
                None => report.game_state.remove(SERVER_KIND),
            };
            assert_eq!(activity(&report), expected);
        }
        report.game_state.insert(DIMENSION.into(), "nether".into());
        report.game_state.insert(SERVER_PACKS.into(), "3".into());
        assert_eq!(session(&report), "32 min, in the Nether, 3 server packs");
    }

    #[test]
    fn startup_failures_show_their_error() {
        let report = report(Kind::Startup, "missing carrier at C:/x; run `make assets`");
        let summary = summary(&report, &Logs::default(), PathBuf::new());
        assert!(matches!(summary.headline, Headline::CouldNotStart));
        assert_eq!(summary.fault, "missing carrier at C:/x; run `make assets`");
        assert_eq!(summary.location, "Before the game started");
    }

    #[test]
    fn runtime_failures_say_what_the_player_was_doing() {
        let mut report = report(Kind::Runtime, "network session stopped unexpectedly");
        report
            .game_state
            .insert(SESSION_PHASE.into(), SessionPhase::InWorld.name());
        report
            .game_state
            .insert(SERVER_KIND.into(), ServerKind::Server.name());
        let summary = summary(&report, &Logs::default(), PathBuf::new());
        assert_eq!(
            summary.headline,
            Headline::Stopped {
                activity: "playing on a server".into()
            }
        );
        assert_eq!(summary.fault, "network session stopped unexpectedly");
        assert_eq!(summary.location, "While the game was running");
    }

    #[test]
    fn the_log_tail_keeps_the_newest_lines() {
        let log: String = (0..250).map(|line| format!("line {line}\n")).collect();
        let tail = log_tail(log.as_bytes());
        assert_eq!(tail.len(), LOG_TAIL_LINES);
        assert_eq!(tail.last().map(String::as_str), Some("line 249"));
    }
}
