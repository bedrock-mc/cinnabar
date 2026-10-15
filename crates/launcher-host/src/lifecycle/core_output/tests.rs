use std::sync::Mutex;

use super::*;

/// A sink tests read back while the writer thread appends to it.
#[derive(Clone, Default)]
pub(crate) struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Buffer {
    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(
            &self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .into_owned()
    }
}

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn category_levels_map_to_the_four_go_levels() {
    assert_eq!(go_log_level(LevelFilter::TRACE), "debug");
    assert_eq!(go_log_level(LevelFilter::DEBUG), "debug");
    assert_eq!(go_log_level(LevelFilter::INFO), "info");
    assert_eq!(go_log_level(LevelFilter::WARN), "warn");
    assert_eq!(go_log_level(LevelFilter::ERROR), "error");
    assert_eq!(go_log_level(LevelFilter::OFF), "error");
}

#[test]
fn only_unrequested_abnormal_exits_and_go_crash_output_are_crashes() {
    assert!(!crashed(Some(0), false, false));
    assert!(
        !crashed(Some(1), false, false),
        "status 1 is the core's own error path"
    );
    assert!(
        crashed(Some(2), false, false),
        "a Go panic without crash output"
    );
    assert!(crashed(None, false, false), "a signal nobody sent");
    assert!(!crashed(None, true, false), "a requested kill");
    assert!(!crashed(Some(1), true, false));
    assert!(
        crashed(Some(2), true, true),
        "a panic while stopping still crashed"
    );
}

#[test]
fn lines_without_a_process_attribute_get_the_label() {
    let label = "process=catalog pid=7";
    assert_eq!(
        labelled(label, b"panic: boom"),
        b"process=catalog pid=7 panic: boom\n"
    );
    let go = b"time=t level=INFO msg=\"started\" process=catalog";
    let mut expected = go.to_vec();
    expected.push(b'\n');
    assert_eq!(labelled(label, go), expected);
    assert_eq!(labelled(label, b"process=game x"), b"process=game x\n");
}

#[test]
fn long_lines_split_and_line_endings_drop() {
    let mut input = b"first\r\n".to_vec();
    input.extend(std::iter::repeat_n(b'x', MAX_LINE_BYTES + 3));
    input.extend_from_slice(b"\nlast");
    let mut lines = Vec::new();
    for_each_line(input.as_slice(), |line| lines.push(line.to_vec()));
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0], b"first");
    assert_eq!(lines[1].len(), MAX_LINE_BYTES);
    assert_eq!(lines[2], b"xxx");
    assert_eq!(lines[3], b"last");
}

/// Blocks its first write until released, standing in for a stalled disk.
struct Stalled {
    entered: Sender<()>,
    release: Receiver<()>,
    out: Buffer,
    stalled: bool,
}

impl Write for Stalled {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.stalled {
            self.stalled = true;
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
        }
        self.out.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_stalled_log_drops_and_counts_lines_without_blocking_writers() {
    let (entered, entering) = bounded(1);
    let (released, release) = bounded(1);
    let out = Buffer::default();
    let log = CoreLog::start_with(
        Box::new(Stalled {
            entered,
            release,
            out: out.clone(),
            stalled: false,
        }),
        2,
    )
    .unwrap();
    log.line(b"a\n".to_vec());
    entering.recv_timeout(test_time::DEFAULT_TIMEOUT).unwrap();
    for line in ["b\n", "c\n", "d\n", "e\n"] {
        log.line(line.as_bytes().to_vec());
    }
    released.send(()).unwrap();
    assert!(log.flush());
    log.line(b"f\n".to_vec());
    assert!(log.flush());
    // The writer reports the drops at the first entry it takes after them.
    assert_eq!(
        out.text(),
        "a\nmsg=\"core log lines dropped\" count=2\nb\nc\nf\n"
    );
}

/// Output routed into `log`, staging crash output in `staging`, counting reports.
fn output(log: &Buffer, staging: &Path, reports: &Arc<AtomicU64>) -> CoreOutput {
    let reports = Arc::clone(reports);
    let report: Reporter = Arc::new(move |_: &CoreCrash| {
        reports.fetch_add(1, Ordering::SeqCst);
        None
    });
    CoreOutput::new(
        || LevelFilter::DEBUG,
        Some(Box::new(log.clone())),
        Some(staging.to_owned()),
        Some(report),
    )
    .unwrap()
}

/// Waits until `pid`'s exit line is logged and its staged crash output is gone, which the
/// watcher does only after handling the exit.
fn wait_for_exit_handled(log: &Buffer, staging: &Path, pid: u32) {
    let exited = format!("process=catalog pid={pid} msg=\"core exited\"");
    test_time::eventually("the helper's exit to be handled", || {
        log.text().contains(&exited)
            && fs::read_dir(staging).is_ok_and(|mut entries| entries.next().is_none())
    });
}

#[test]
fn helper_stderr_reaches_the_core_log_labelled_with_its_role_and_pid() {
    let Some(core) = fixture::crash_core() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join(STAGING_DIR);
    let (log, reports) = (Buffer::default(), Arc::new(AtomicU64::new(0)));
    let output = output(&log, &staging, &reports);
    for mode in ["clean", "fail"] {
        let child = output
            .spawn(&mut fixture::command(core, mode), CoreRole::Catalog)
            .unwrap();
        let pid = child.id();
        child.wait();
        wait_for_exit_handled(&log, &staging, pid);
        let text = log.text();
        assert!(text.contains(&format!("process=catalog pid={pid} raw fixture line\n")));
        assert!(
            text.contains("level=DEBUG msg=\"fixture debug record\" process=fixture"),
            "-log-level carries the category's debug level:\n{text}"
        );
        assert!(
            !text.contains(&format!("process=catalog pid={pid} time=")),
            "a record naming its process keeps its own label"
        );
    }
    assert_eq!(
        reports.load(Ordering::SeqCst),
        0,
        "neither a clean exit nor the core's own failure is a crash"
    );
}

#[test]
fn uninstalled_spawns_pass_the_default_level_and_no_crash_output() {
    let mut command = Command::new("bedrock-core-that-does-not-exist");
    assert!(
        CoreOutput::uninstalled()
            .spawn(&mut command, CoreRole::Game)
            .is_err()
    );
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(args, ["-log-level", "info"]);
}
