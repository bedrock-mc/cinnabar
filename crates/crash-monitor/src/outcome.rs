//! How the client ended, from what it said and its exit status, and the report that follows.

use std::path::{Path, PathBuf};

use crash_report::{
    folder::{self, LogSources, Logs},
    native::Address,
    report::{Identity, Kind, Report, Thread},
};
use launcher_host::lifecycle::crash::{
    attachments::Attachments,
    protocol::{self, Hello, Panic},
};

use crate::{
    dump,
    server::{DumpedPanic, Failure, Observed},
};

/// Exit codes of a deliberate exit that skipped its announcement: success, and Ctrl+C.
const CLEAN_EXIT_CODES: [u32; 2] = [0, 130];
/// Panics at most this far apart are one failure spreading across threads.
pub(crate) const CASCADE_MS: u64 = 1_000;

/// How the client ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ending {
    /// It exited on purpose; nothing more to report.
    Clean,
    /// It asked for a dump for a native fault, or announced a panic whose dump never came.
    Crash,
    /// It died after a panic whose report was written when its dump arrived.
    Panicked,
    /// `main` failed with this error.
    Failed(Failure),
    /// It died without a dump: an abort, a fail-fast or a kill. The exit code when known.
    Abort(Option<u32>),
    /// It never introduced itself, so there is nobody to report on.
    Unknown,
}

/// Classifies the client's end; `panicked` says a panic's report was already written, and
/// `exit_code` is the exit status where the platform lets a non-parent read it (Windows).
pub(crate) fn ending(observed: &Observed, panicked: bool, exit_code: Option<u32>) -> Ending {
    if observed.hello.is_none() {
        return Ending::Unknown;
    }
    if observed.panic.is_some() || observed.dump.is_some() {
        return Ending::Crash;
    }
    if let Some(failure) = &observed.failure {
        return Ending::Failed(failure.clone());
    }
    if panicked && panic_exit(observed.exit, exit_code) {
        return Ending::Panicked;
    }
    if observed.exit.is_some() {
        return Ending::Clean;
    }
    match exit_code {
        Some(code) if CLEAN_EXIT_CODES.contains(&code) => Ending::Clean,
        code => Ending::Abort(code),
    }
}

/// A saved panic explains termination only when the final evidence matches a panic exit.
/// Unix's return hook has no exit status, while Windows can verify the runtime's code 101.
fn panic_exit(exit: Option<i32>, exit_code: Option<u32>) -> bool {
    if exit_code.is_some_and(|code| code != u32::from(protocol::PANIC_EXIT_CODE)) {
        return false;
    }
    exit == Some(protocol::AT_EXIT)
        || exit == Some(i32::from(protocol::PANIC_EXIT_CODE))
        || (exit.is_none() && exit_code.is_some())
}

/// Builds the report for an `ending` that needs a new folder at `now_ms`; `None` for clean,
/// unknown and already reported ends.
pub(crate) fn report(observed: &Observed, ending: &Ending, now_ms: u64) -> Option<Report> {
    let hello = observed.hello.as_ref()?;
    let identity = &hello.identity;
    let mut report = match ending {
        Ending::Clean | Ending::Panicked | Ending::Unknown => return None,
        Ending::Crash => crash_report(observed, now_ms)?,
        Ending::Failed(failure) => {
            Report::new(failure.kind, failure.error.clone(), identity, now_ms)
        }
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

/// The report for a panic whose dump arrived while the client lived, with its state then.
pub(crate) fn dumped_panic_report(hello: &Hello, dumped: &DumpedPanic, now_ms: u64) -> Report {
    let mut report = panic_report(&hello.identity, &dumped.panic, now_ms);
    dumped.state.apply_to(&mut report);
    report
}

fn panic_report(identity: &Identity, panic: &Panic, now_ms: u64) -> Report {
    let mut report = Report::new(Kind::Panic, panic.message.clone(), identity, now_ms);
    report.thread = panic.thread.clone();
    report.backtrace = panic.backtrace.clone();
    report.uptime_ms = panic.uptime_ms;
    report
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
        return Some(panic_report(identity, panic, now_ms));
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

/// Writes `report` with the client's log sections as they are now and the attachments of its
/// staging directory, moving `dump` in.
pub(crate) fn write(
    crash_dir: &Path,
    hello: &Hello,
    report: &Report,
    dump: Option<&Path>,
    now_ms: u64,
) -> std::io::Result<(PathBuf, Logs)> {
    let logs = Logs::read(&LogSources {
        session_id: Some(&hello.identity.session_id),
        client: &hello.client_log,
        core: &hello.core_log,
        started_at_ms: hello.identity.started_at_ms,
    });
    let attachments = hello
        .staging
        .as_deref()
        .map(Attachments::read)
        .unwrap_or_default();
    let folder = folder::write_with(
        crash_dir,
        report,
        &logs,
        dump,
        &attachments.as_folder(),
        now_ms,
    )?;
    if let Some(stack) = folder
        .join(folder::DUMP_FILE)
        .is_file()
        .then(|| dump::stack(&folder.join(folder::DUMP_FILE)))
        .flatten()
        && let Err(error) = crash_report::stack::write(&folder, &stack)
    {
        tracing::warn!("native frame metadata could not be written: {error}");
    }
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
    use launcher_host::lifecycle::crash::protocol::State;

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
            full_dump: false,
            staging: None,
        }
    }

    fn introduced() -> Observed {
        Observed {
            hello: Some(hello()),
            ..Observed::default()
        }
    }

    fn panic() -> Panic {
        Panic {
            message: "panicked at src/main.rs:1:1:\nboom".into(),
            thread: Thread {
                name: Some("main".into()),
                id: "ThreadId(1)".into(),
            },
            backtrace: "0: main".into(),
            uptime_ms: 2_000,
        }
    }

    #[test]
    fn deliberate_exits_report_nothing() {
        let mut observed = introduced();
        assert_eq!(ending(&observed, false, Some(0)), Ending::Clean);
        assert_eq!(ending(&observed, false, Some(130)), Ending::Clean);
        observed.exit = Some(1);
        assert_eq!(ending(&observed, false, Some(1)), Ending::Clean);
        assert_eq!(ending(&observed, false, None), Ending::Clean);
        observed.exit = Some(protocol::SELF_REPORTED);
        assert_eq!(ending(&observed, false, Some(101)), Ending::Clean);
        assert_eq!(report(&observed, &Ending::Clean, 5_000), None);
    }

    #[test]
    fn exits_without_a_dump_or_announcement_are_aborts() {
        let observed = introduced();
        assert_eq!(
            ending(&observed, false, Some(0xC000_0409)),
            Ending::Abort(Some(0xC000_0409))
        );
        assert_eq!(ending(&observed, false, None), Ending::Abort(None));
        let report = report(&observed, &Ending::Abort(Some(0xC000_0409)), 5_000).unwrap();
        assert_eq!(report.kind, Kind::Abort);
        assert!(report.message.contains("0xC0000409"), "{}", report.message);
        assert_eq!(report.uptime_ms, 4_000);
    }

    #[test]
    fn a_reported_panic_shows_only_when_the_client_then_dies() {
        let mut observed = introduced();
        // The runtime's exit after a main-thread panic: 101 on Windows, `atexit` on Unix.
        assert_eq!(ending(&observed, true, Some(101)), Ending::Panicked);
        observed.exit = Some(protocol::AT_EXIT);
        assert_eq!(ending(&observed, true, None), Ending::Panicked);
        // A teardown the watchdog cut short.
        observed.exit = Some(101);
        assert_eq!(ending(&observed, true, Some(101)), Ending::Panicked);
        // A thread panicked, then the player quit.
        observed.exit = Some(0);
        assert_eq!(ending(&observed, true, None), Ending::Clean);
        observed.exit = None;
        assert_eq!(ending(&observed, true, Some(0)), Ending::Clean);
        assert_eq!(report(&observed, &Ending::Panicked, 5_000), None);
    }

    #[test]
    fn diagnostics_audit_recovered_panics_do_not_hide_later_terminations() {
        let mut observed = introduced();
        for code in [None, Some(9), Some(137), Some(0xC000_0409)] {
            assert_eq!(ending(&observed, true, code), Ending::Abort(code));
        }
        observed.exit = Some(protocol::SELF_REPORTED);
        assert_eq!(ending(&observed, true, None), Ending::Clean);
    }

    #[test]
    fn failures_carry_their_stage() {
        let mut observed = introduced();
        for kind in [Kind::Startup, Kind::Runtime] {
            let failure = Failure {
                kind,
                error: "missing carrier".into(),
            };
            observed.failure = Some(failure.clone());
            observed.exit = Some(1);
            let ending = ending(&observed, true, Some(1));
            assert_eq!(ending, Ending::Failed(failure));
            let report = report(&observed, &ending, 5_000).unwrap();
            assert_eq!(report.kind, kind);
            assert_eq!(report.message, "missing carrier");
        }
    }

    #[test]
    fn crashes_outrank_failures_and_exits() {
        let mut observed = introduced();
        observed.failure = Some(Failure {
            kind: Kind::Startup,
            error: "missing carrier".into(),
        });
        observed.exit = Some(1);
        observed.panic = Some(panic());
        assert_eq!(ending(&observed, false, Some(101)), Ending::Crash);
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
    fn dumped_panics_keep_the_state_they_had() {
        let dumped = DumpedPanic {
            panic: panic(),
            dump: Dump {
                path: None,
                at_ms: 3_000,
            },
            state: State {
                gpu: Some("GPU".into()),
                ..State::default()
            },
        };
        let report = dumped_panic_report(&hello(), &dumped, 9_000);
        assert_eq!(report.kind, Kind::Panic);
        assert_eq!(report.uptime_ms, 2_000);
        assert_eq!(report.gpu.as_deref(), Some("GPU"));
    }

    #[test]
    fn clients_that_never_connected_are_not_reported() {
        assert_eq!(
            ending(&Observed::default(), false, Some(3)),
            Ending::Unknown
        );
    }
}
