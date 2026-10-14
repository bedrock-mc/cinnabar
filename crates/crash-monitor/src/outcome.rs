//! How the client ended, from what it said and its exit status, and the report that follows.

use std::path::{Path, PathBuf};

use crash_report::{
    folder::{self, LogSources, Logs},
    native::Address,
    report::{Kind, Report, Thread},
};

use crate::{dump, server::Observed};

/// Exit codes of a deliberate exit that skipped its announcement: success, and Ctrl+C.
const CLEAN_EXIT_CODES: [u32; 2] = [0, 130];

/// How the client ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ending {
    /// It exited on purpose; nothing to report.
    Clean,
    /// It asked for a dump, after a panic or a native fault.
    Crash,
    /// `main` failed with this error before the game started.
    Startup(String),
    /// It died without a dump: an abort, a fail-fast or a kill. The exit code when known.
    Abort(Option<u32>),
    /// It never introduced itself, so there is nobody to report on.
    Unknown,
}

/// Classifies the client's end; `exit_code` is its exit status where the platform lets a
/// non-parent read it (Windows).
pub(crate) fn ending(observed: &Observed, exit_code: Option<u32>) -> Ending {
    if observed.hello.is_none() {
        return Ending::Unknown;
    }
    if observed.panic.is_some() || observed.dump.is_some() {
        return Ending::Crash;
    }
    if let Some(error) = &observed.startup_failure {
        return Ending::Startup(error.clone());
    }
    if observed.exit.is_some() {
        return Ending::Clean;
    }
    match exit_code {
        Some(code) if CLEAN_EXIT_CODES.contains(&code) => Ending::Clean,
        code => Ending::Abort(code),
    }
}

/// Builds the report for a non-clean `ending` at `now_ms`; `None` for clean and unknown ends.
pub(crate) fn report(observed: &Observed, ending: &Ending, now_ms: u64) -> Option<Report> {
    let hello = observed.hello.as_ref()?;
    let identity = &hello.identity;
    let mut report = match ending {
        Ending::Clean | Ending::Unknown => return None,
        Ending::Crash => crash_report(observed, now_ms)?,
        Ending::Startup(error) => Report::new(Kind::Startup, error.clone(), identity, now_ms),
        Ending::Abort(code) => {
            let mut report = Report::new(Kind::Abort, abort_message(*code), identity, now_ms);
            report.native =
                code.map(|code| serde_json::json!({ "exit_code": format!("0x{code:08X}") }));
            report
        }
    };
    observed.state.apply_to(&mut report);
    Some(report)
}

fn abort_message(code: Option<u32>) -> String {
    match code {
        Some(code) => format!("Exited with code 0x{code:08X} without a crash dump"),
        None => "Ended without a crash dump: aborted or killed".to_owned(),
    }
}

/// A panic or native report; the dump says where a native fault happened.
fn crash_report(observed: &Observed, now_ms: u64) -> Option<Report> {
    let identity = &observed.hello.as_ref()?.identity;
    if let Some(panic) = &observed.panic {
        let mut report = Report::new(Kind::Panic, panic.message.clone(), identity, now_ms);
        report.thread = panic.thread.clone();
        report.backtrace = panic.backtrace.clone();
        report.uptime_ms = panic.uptime_ms;
        return Some(report);
    }
    let dump = observed.dump.as_ref()?;
    let fault = dump.path.as_deref().and_then(dump::fault);
    let message = fault
        .as_ref()
        .map_or_else(|| "Native crash".to_owned(), |fault| fault.reason.clone());
    let mut report = Report::new(Kind::Native, message, identity, dump.at_ms.min(now_ms));
    if let Some(fault) = fault {
        report.thread = Thread {
            name: fault.thread_name.clone(),
            id: fault.thread_id.to_string(),
        };
        report.native = Some(serde_json::json!({
            "reason": fault.reason,
            "address": format!("0x{:X}", fault.address),
            "fault": fault.instruction,
        }));
    }
    Some(report)
}

/// Writes `report` with the client's log sections, read now that it is gone, and its dump.
pub(crate) fn write(
    crash_dir: &Path,
    observed: &Observed,
    report: &Report,
    now_ms: u64,
) -> std::io::Result<(PathBuf, Logs)> {
    let hello = observed
        .hello
        .as_ref()
        .ok_or_else(|| std::io::Error::other("the client never introduced itself"))?;
    let logs = Logs::read(&LogSources {
        client: &hello.client_log,
        core: &hello.core_log,
        started_at_ms: hello.identity.started_at_ms,
    });
    let dump = observed.dump.as_ref().and_then(|dump| dump.path.as_deref());
    let folder = folder::write(crash_dir, report, &logs, dump, now_ms)?;
    Ok((folder, logs))
}

/// Where a native fault's instruction was, for the window.
pub(crate) fn native_location(report: &Report) -> Option<String> {
    let fault = report.native.as_ref()?.get("fault")?;
    let address: Address = serde_json::from_value(fault.clone()).ok()?;
    Some(address.location())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::Dump;
    use crash_report::report::Identity;
    use launcher_host::lifecycle::crash::protocol::{Hello, Panic, Restart};

    fn hello() -> Hello {
        Hello {
            identity: Identity {
                session_id: "0123456789abcdef".into(),
                started_at_ms: 1_000,
                release: "1.2.3".into(),
                commit: "abc123".into(),
                build_profile: "release".into(),
            },
            client_log: PathBuf::from("client.log"),
            core_log: PathBuf::from("core.log"),
            restart: Restart {
                program: PathBuf::from("cinnabar"),
                args: Vec::new(),
                env_remove: Vec::new(),
            },
            full_dump: false,
        }
    }

    fn introduced() -> Observed {
        Observed {
            hello: Some(hello()),
            ..Observed::default()
        }
    }

    #[test]
    fn deliberate_exits_report_nothing() {
        let mut observed = introduced();
        assert_eq!(ending(&observed, Some(0)), Ending::Clean);
        assert_eq!(ending(&observed, Some(130)), Ending::Clean);
        observed.exit = Some(1);
        assert_eq!(ending(&observed, Some(1)), Ending::Clean);
        assert_eq!(ending(&observed, None), Ending::Clean);
        assert_eq!(report(&observed, &Ending::Clean, 5_000), None);
    }

    #[test]
    fn exits_without_a_dump_or_announcement_are_aborts() {
        let observed = introduced();
        assert_eq!(
            ending(&observed, Some(0xC000_0409)),
            Ending::Abort(Some(0xC000_0409))
        );
        assert_eq!(ending(&observed, None), Ending::Abort(None));
        let report = report(&observed, &Ending::Abort(Some(0xC000_0409)), 5_000).unwrap();
        assert_eq!(report.kind, Kind::Abort);
        assert!(report.message.contains("0xC0000409"), "{}", report.message);
        assert_eq!(report.uptime_ms, 4_000);
    }

    #[test]
    fn crashes_outrank_startup_failures_and_exits() {
        let mut observed = introduced();
        observed.startup_failure = Some("missing carrier".into());
        observed.exit = Some(1);
        assert_eq!(
            ending(&observed, Some(1)),
            Ending::Startup("missing carrier".into())
        );
        let startup = report(&observed, &ending(&observed, Some(1)), 5_000).unwrap();
        assert_eq!(startup.kind, Kind::Startup);
        assert_eq!(startup.message, "missing carrier");
        observed.panic = Some(Panic {
            message: "panicked at src/main.rs:1:1:\nboom".into(),
            thread: Thread {
                name: Some("main".into()),
                id: "ThreadId(1)".into(),
            },
            backtrace: "0: main".into(),
            uptime_ms: 2_000,
        });
        observed.dump = Some(Dump {
            path: None,
            at_ms: 3_000,
        });
        assert_eq!(ending(&observed, Some(101)), Ending::Crash);
        let panic = report(&observed, &Ending::Crash, 5_000).unwrap();
        assert_eq!(panic.kind, Kind::Panic);
        assert_eq!(panic.thread.name.as_deref(), Some("main"));
        assert_eq!(panic.uptime_ms, 2_000);
    }

    #[test]
    fn a_dump_without_a_panic_is_native() {
        let mut observed = introduced();
        observed.dump = Some(Dump {
            path: None,
            at_ms: 3_000,
        });
        observed.state.gpu = Some("GPU".into());
        let report = report(&observed, &Ending::Crash, 5_000).unwrap();
        assert_eq!(report.kind, Kind::Native);
        assert_eq!(report.uptime_ms, 2_000);
        assert_eq!(report.gpu.as_deref(), Some("GPU"));
    }

    #[test]
    fn clients_that_never_connected_are_not_reported() {
        assert_eq!(ending(&Observed::default(), Some(3)), Ending::Unknown);
    }
}
