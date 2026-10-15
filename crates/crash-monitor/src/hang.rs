//! Hang detection: the client's heartbeat stops advancing while no loading phase is open.
//!
//! [`Watch`] decides from samples and caller-supplied times; [`spawn`] samples the client's
//! heartbeat on a thread and hands each stall to the caller, which writes its report while the
//! client keeps running.

use std::{
    path::Path,
    sync::mpsc::{self, RecvTimeoutError},
    thread::JoinHandle,
    time::Duration,
};

use crash_report::report::{Identity, Kind, Report, Thread};
use launcher_host::lifecycle::crash::{
    heartbeat::{Heartbeat, Region},
    protocol::State,
};

/// Heartbeat samples per stall window, so a stall is seen within a tenth of its length.
pub const HANG_SAMPLES_PER_WINDOW: u64 = 10;
/// The shortest sampling interval, in milliseconds.
const MIN_SAMPLE_MS: u64 = 10;
/// The name hang reports give the watched thread, which the client's main loop runs on.
const WATCHED_THREAD: &str = "main";

/// What the monitor read from the heartbeat.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Sample {
    pub frame: u64,
    pub loading: bool,
}

/// A stall long enough to be a hang.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Stall {
    /// The last frame the watched thread finished.
    pub frame: u64,
    /// How long it has gone without progress.
    pub stalled_ms: u64,
}

/// A change [`Watch::observe`] reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Change {
    /// A new stall; reported once until progress resumes.
    Stalled(Stall),
    /// The stalled thread finished a frame again.
    Resumed,
}

/// Decides when the heartbeat has stalled, from samples taken at caller-supplied times.
#[derive(Clone, Debug)]
pub(crate) struct Watch {
    stall_after_ms: u64,
    frame: u64,
    /// When the frame last advanced, a loading phase was last open, or sampling resumed after a
    /// gap.
    progress_at_ms: u64,
    sampled_at_ms: u64,
    stalled: bool,
}

impl Watch {
    /// Starts watching at `now_ms`; `stall_after_ms` without progress outside loading is a hang.
    pub(crate) fn new(stall_after_ms: u64, now_ms: u64) -> Self {
        Self {
            stall_after_ms,
            frame: 0,
            progress_at_ms: now_ms,
            sampled_at_ms: now_ms,
            stalled: false,
        }
    }

    /// Records `sample` taken at `now_ms`.
    ///
    /// Frame `0` means the client has not finished a frame yet, which is never a hang. A gap
    /// between samples of over half the stall window means the monitor itself was not running,
    /// as across system sleep, so the stall restarts from now.
    pub(crate) fn observe(&mut self, sample: Sample, now_ms: u64) -> Option<Change> {
        let gap_ms = now_ms.saturating_sub(self.sampled_at_ms);
        self.sampled_at_ms = now_ms;
        if sample.frame != self.frame {
            self.frame = sample.frame;
            self.progress_at_ms = now_ms;
            return std::mem::take(&mut self.stalled).then_some(Change::Resumed);
        }
        if sample.frame == 0 || sample.loading || gap_ms > self.stall_after_ms / 2 {
            self.progress_at_ms = now_ms;
            return None;
        }
        let stalled_ms = now_ms.saturating_sub(self.progress_at_ms);
        if self.stalled || stalled_ms < self.stall_after_ms {
            return None;
        }
        self.stalled = true;
        Some(Change::Stalled(Stall {
            frame: sample.frame,
            stalled_ms,
        }))
    }
}

/// How often to sample for a `stall_after_ms` window.
fn sample_interval(stall_after_ms: u64) -> Duration {
    Duration::from_millis((stall_after_ms / HANG_SAMPLES_PER_WINDOW).max(MIN_SAMPLE_MS))
}

/// A running hang watchdog; [`Watchdog::stop`] ends it.
pub(crate) struct Watchdog<T> {
    stop: mpsc::Sender<()>,
    thread: JoinHandle<Option<T>>,
}

impl<T> Watchdog<T> {
    /// Stops sampling and returns what the stall in progress produced, if the client was still
    /// stalled.
    pub(crate) fn stop(self) -> Option<T> {
        drop(self.stop);
        self.thread.join().ok().flatten()
    }
}

/// Samples `heartbeat` on a thread, timing it with `now_ms`. Each new stall goes to `on_stall`
/// with the heartbeat's region; what it returns is kept until progress resumes, and handed back
/// by [`Watchdog::stop`] when the client was still stalled then. The monitor's sample count in
/// the region advances after each sample is acted on.
pub(crate) fn spawn<T, F>(
    heartbeat: Heartbeat,
    stall_after_ms: u64,
    now_ms: fn() -> u64,
    mut on_stall: F,
) -> std::io::Result<Watchdog<T>>
where
    T: Send + 'static,
    F: FnMut(&Region, Stall) -> Option<T> + Send + 'static,
{
    let (stop, stopped) = mpsc::channel::<()>();
    let interval = sample_interval(stall_after_ms);
    let thread = std::thread::Builder::new()
        .name("hang-watchdog".into())
        .spawn(move || {
            let region = heartbeat.region();
            let mut watch = Watch::new(stall_after_ms, now_ms());
            let mut stalled = None;
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(interval) {
                let sample = Sample {
                    frame: region.frame(),
                    loading: region.loading(),
                };
                match watch.observe(sample, now_ms()) {
                    Some(Change::Stalled(stall)) => {
                        tracing::warn!(
                            "client not responding: no frame for {} ms after frame {}",
                            stall.stalled_ms,
                            stall.frame
                        );
                        stalled = on_stall(region, stall);
                    }
                    Some(Change::Resumed) => {
                        tracing::info!("client responding again");
                        stalled = None;
                    }
                    None => {}
                }
                region.record_check();
            }
            stalled
        })?;
    Ok(Watchdog { stop, thread })
}

/// The report for `stall` of the client `identity` names, in `state`, on its watched thread
/// `thread`; `missing_dump` says why no minidump accompanies it.
pub(crate) fn report(
    identity: &Identity,
    state: &State,
    stall: Stall,
    thread: u64,
    missing_dump: Option<&str>,
    now_ms: u64,
) -> Report {
    let mut message = format!(
        "Not responding: the main thread finished no frame for {} s after frame {}",
        stall.stalled_ms / 1_000,
        stall.frame
    );
    if let Some(reason) = missing_dump {
        message.push_str(&format!("; no minidump: {reason}"));
    }
    let mut report = Report::new(Kind::Hang, message, identity, now_ms);
    report.thread = Thread {
        name: Some(WATCHED_THREAD.to_owned()),
        id: thread.to_string(),
    };
    state.apply_to(&mut report);
    report
}

/// Writes a minidump of every thread of the live process `pid` to `path`, blaming `thread`,
/// without ending the process; with all its memory when `full_memory` (Windows only).
#[cfg(windows)]
pub(crate) fn dump_live(
    pid: u32,
    thread: u64,
    full_memory: bool,
    path: &Path,
) -> Result<(), String> {
    use minidump_writer::{
        MinidumpType, crash_context::CrashContext, minidump_writer::MinidumpWriter,
    };

    let crash = CrashContext {
        exception_pointers: std::ptr::null(),
        exception_code: 0,
        process_id: pid,
        thread_id: u32::try_from(thread).unwrap_or(0),
    };
    let mut file = crate::dump::create(path).map_err(|error| error.to_string())?;
    MinidumpWriter::dump_crash_context(
        &crash,
        full_memory.then_some(MinidumpType::WithFullMemory),
        &mut file,
    )
    .map_err(|error| error.to_string())
}

/// Writes a minidump of every thread of the live process `pid` to `path`, blaming `thread`,
/// without ending the process. Needs the client's `PR_SET_PTRACER` permission under Yama.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn dump_live(
    pid: u32,
    thread: u64,
    _full_memory: bool,
    path: &Path,
) -> Result<(), String> {
    use minidump_writer::minidump_writer::MinidumpWriterConfig;

    let pid = i32::try_from(pid).map_err(|error| error.to_string())?;
    let thread = i32::try_from(thread).unwrap_or(pid);
    let mut file = crate::dump::create(path).map_err(|error| error.to_string())?;
    MinidumpWriterConfig::new(pid, thread)
        .write(&mut file)
        .map(drop)
        .map_err(|error| error.to_string())
}

/// macOS gives a task port only to privileged processes, so the monitor cannot dump the live
/// client.
#[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
pub(crate) fn dump_live(
    _pid: u32,
    _thread: u64,
    _full_memory: bool,
    _path: &Path,
) -> Result<(), String> {
    Err(format!(
        "dumping a running process is not supported on {}",
        std::env::consts::OS
    ))
}

/// Whether a debugger is attached to the process `pid`, which pauses it on purpose.
#[cfg(windows)]
pub(crate) fn debugger_attached(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Debug::CheckRemoteDebuggerPresent,
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION},
        },
    };

    // SAFETY: Opening a process handle by id has no other preconditions; the handle is closed
    // once below, and `present` is a valid out pointer.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut present = 0;
        let checked = CheckRemoteDebuggerPresent(handle, &mut present) != 0;
        CloseHandle(handle);
        checked && present != 0
    }
}

/// Whether a debugger is attached to the process `pid`, which pauses it on purpose.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn debugger_attached(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/status")).is_ok_and(|status| {
        status
            .lines()
            .find_map(|line| line.strip_prefix("TracerPid:"))
            .is_some_and(|tracer| tracer.trim() != "0")
    })
}

/// Whether a debugger is attached to the process `pid`, which pauses it on purpose: the
/// kernel's traced flag, which a same-user process may read as `ps` does. `proc_pidinfo`'s
/// `PROC_FLAG_TRACED` and `sysctl`'s `kinfo_proc` `P_TRACED` both report the process's
/// `P_LTRACED`; the former has typed bindings.
#[cfg(target_os = "macos")]
pub(crate) fn debugger_attached(pid: u32) -> bool {
    /// `PROC_FLAG_TRACED` from `<sys/proc_info.h>`.
    const PROC_FLAG_TRACED: u32 = 0x2;
    let Ok(pid) = libc::c_int::try_from(pid) else {
        return false;
    };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    // SAFETY: `info` is a writable `proc_bsdinfo` of `size` bytes, the size this flavor fills.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    // SAFETY: Every field is an integer or integer array, so the zeroed value is valid.
    written == size && unsafe { info.assume_init() }.pbi_flags & PROC_FLAG_TRACED != 0
}

/// Whether a debugger is attached to the process `pid`; not detectable from another process
/// here, so never.
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos"
)))]
pub(crate) fn debugger_attached(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn failed_hang_dumps_are_private_from_creation() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("hang.dmp");
        assert!(dump_live(i32::MAX as u32, 0, false, &path).is_err());
        let mode = std::fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0);
    }

    const STALL_MS: u64 = 10_000;
    const STEP_MS: u64 = 1_000;

    /// Feeds `watch` one sample per [`STEP_MS`] of a manual clock.
    struct Clock {
        watch: Watch,
        now_ms: u64,
    }

    impl Clock {
        fn new() -> Self {
            Self {
                watch: Watch::new(STALL_MS, 0),
                now_ms: 0,
            }
        }

        fn step(&mut self, frame: u64, loading: bool) -> Option<Change> {
            self.now_ms += STEP_MS;
            self.watch.observe(Sample { frame, loading }, self.now_ms)
        }

        /// Steps `count` times with the same sample, collecting every change.
        fn hold(&mut self, count: usize, frame: u64, loading: bool) -> Vec<Change> {
            (0..count)
                .filter_map(|_| self.step(frame, loading))
                .collect()
        }
    }

    #[test]
    fn a_stall_is_reported_once_after_the_window() {
        let mut clock = Clock::new();
        assert_eq!(clock.step(7, false), None);
        assert_eq!(clock.hold(9, 7, false), Vec::new());
        assert_eq!(
            clock.step(7, false),
            Some(Change::Stalled(Stall {
                frame: 7,
                stalled_ms: STALL_MS
            }))
        );
        assert_eq!(clock.hold(50, 7, false), Vec::new());
    }

    #[test]
    fn progress_keeps_the_watch_quiet() {
        let mut clock = Clock::new();
        let changes: Vec<Change> = (1..=100)
            .filter_map(|frame| clock.step(frame, false))
            .collect();
        assert_eq!(changes, Vec::new());
    }

    #[test]
    fn loading_suppresses_a_stall_and_the_window_restarts_when_it_ends() {
        let mut clock = Clock::new();
        clock.step(3, false);
        assert_eq!(clock.hold(60, 3, true), Vec::new());
        assert_eq!(clock.hold(9, 3, false), Vec::new());
        assert!(matches!(clock.step(3, false), Some(Change::Stalled(_))));
    }

    #[test]
    fn nothing_counts_before_the_first_frame() {
        let mut clock = Clock::new();
        assert_eq!(clock.hold(60, 0, false), Vec::new());
    }

    #[test]
    fn a_new_stall_after_recovery_is_reported_again() {
        let mut clock = Clock::new();
        clock.step(1, false);
        assert!(matches!(clock.hold(10, 1, false)[..], [Change::Stalled(_)]));
        assert_eq!(clock.step(2, false), Some(Change::Resumed));
        assert_eq!(clock.step(3, false), None);
        assert_eq!(
            clock.hold(10, 3, false),
            vec![Change::Stalled(Stall {
                frame: 3,
                stalled_ms: STALL_MS
            })]
        );
    }

    #[test]
    fn a_gap_in_sampling_is_not_a_stall() {
        let mut clock = Clock::new();
        clock.step(5, false);
        clock.now_ms += 3_600_000;
        assert_eq!(clock.step(5, false), None);
        assert_eq!(clock.hold(9, 5, false), Vec::new());
        assert!(matches!(clock.step(5, false), Some(Change::Stalled(_))));
    }

    #[test]
    fn the_report_names_the_stall_the_thread_and_a_missing_dump() {
        use launcher_host::lifecycle::crash::state::SESSION_PHASE;

        let identity = Identity {
            session_id: "0123456789abcdef".into(),
            started_at_ms: 1_000,
            release: "1.2.3".into(),
            commit: "abc".into(),
            build_profile: "release".into(),
        };
        let mut state = State::default();
        state
            .game_state
            .insert(SESSION_PHASE.into(), "in_world".into());
        let stall = Stall {
            frame: 42,
            stalled_ms: 12_345,
        };
        let report = report(&identity, &state, stall, 4_242, Some("unsupported"), 61_000);
        assert_eq!(report.kind, Kind::Hang);
        assert!(
            report.message.contains("12 s after frame 42"),
            "{}",
            report.message
        );
        assert!(report.message.contains("no minidump: unsupported"));
        assert_eq!(report.thread.id, "4242");
        assert_eq!(report.uptime_ms, 60_000);
        assert_eq!(report.game_state, state.game_state);
    }
}
