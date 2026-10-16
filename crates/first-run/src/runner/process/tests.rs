use super::*;

use std::{
    os::unix::process::CommandExt,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

use rustix::process::{Pid, Signal, WaitOptions};

use crate::{
    runner::{
        Cancelled,
        tests::{compiler_log, script, shell_compiler},
    },
    test_support::Dir,
};

/// Reports when a pipe reader releases its log, after EOF and its final write.
struct ReaderLog {
    stream: &'static str,
    dropped: mpsc::Sender<&'static str>,
}

impl Write for ReaderLog {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for ReaderLog {
    fn drop(&mut self) {
        let _ = self.dropped.send(self.stream);
    }
}

/// Asserts the owner already reaped this child, cleaning up a regression's live fixture first.
fn assert_reaped(pid: Pid) {
    let status = rustix::process::waitpid(Some(pid), WaitOptions::NOHANG);
    if matches!(status, Ok(None)) {
        stop_fixture(pid);
        let _ = rustix::process::waitpid(Some(pid), WaitOptions::empty());
    }
    assert_eq!(
        status.err(),
        Some(rustix::io::Errno::CHILD),
        "child was not reaped"
    );
}

/// Releases only this test's process and pipe handles if a lifecycle regression blocks cleanup.
fn stop_fixture(pid: Pid) {
    let _ = rustix::process::kill_process_group(pid, Signal::KILL);
    let _ = rustix::process::kill_process(pid, Signal::KILL);
}

#[test]
fn dropping_an_unfinished_compiler_reaps_it_before_joining_both_readers() {
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "printf 'error detail\\n' >&2; printf 'ready\\n'; kill -STOP $$",
        ])
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let (dropped, readers) = mpsc::channel();
    let (child, lines) = CompilerChild::spawn(
        &mut command,
        Box::new(ReaderLog {
            stream: "stdout",
            dropped: dropped.clone(),
        }),
        Box::new(ReaderLog {
            stream: "stderr",
            dropped,
        }),
    )
    .unwrap();
    let pid = Pid::from_raw(child.child.id() as i32).unwrap();
    test_time::eventually("compiler output before its blocked lifetime", || {
        lines.try_recv().is_ok_and(|line| line == "ready")
    });

    let cleanup = thread::spawn(move || drop(child));
    let completed = test_time::wait_until(test_time::DEFAULT_TIMEOUT, || cleanup.is_finished());
    if !completed {
        stop_fixture(pid);
    }
    cleanup.join().unwrap();
    assert_reaped(pid);
    let mut released: Vec<_> = readers.try_iter().collect();
    released.sort_unstable();
    assert_eq!(released, ["stderr", "stdout"]);
    assert!(
        completed,
        "cleanup joined readers before stopping their process"
    );
}

#[test]
fn a_pipe_capture_error_still_reaps_the_child() {
    // Spawn first so the fixture can inspect the PID of an error before any reader starts.
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "kill -STOP $$"]).process_group(0);
    let child = command.spawn().unwrap();
    let pid = Pid::from_raw(child.id() as i32).unwrap();
    let result = {
        let mut running = CompilerChild::new(child);
        running.pipes()
    };
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("capture asset compiler output")
    );
    assert_reaped(pid);
}

#[test]
fn a_status_poll_error_reaps_the_compiler_and_keeps_its_original_source() {
    let dir = Dir::new("compiler-wait-error");
    let cancel = AtomicBool::new(false);
    let compiler = shell_compiler(dir.path(), &cancel);
    let args = script("kill -STOP $$");
    let (started, received) = mpsc::channel();
    let error = thread::scope(|scope| {
        let running = scope.spawn(|| {
            compiler.run_with_wait(
                &args,
                |_| {},
                |child| {
                    started.send(child.id()).unwrap();
                    Err(io::Error::other("injected status polling failure"))
                },
            )
        });
        let pid = Pid::from_raw(received.recv_timeout(test_time::DEFAULT_TIMEOUT).unwrap() as i32)
            .unwrap();
        let completed = test_time::wait_until(test_time::DEFAULT_TIMEOUT, || running.is_finished());
        if !completed {
            stop_fixture(pid);
        }
        let result = running.join().unwrap();
        assert_reaped(pid);
        assert!(completed, "an error waited for live compiler pipe readers");
        result.unwrap_err()
    });
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::Other
    );
    let chain = format!("{error:#}");
    assert!(chain.contains("wait for the asset compiler"), "{chain}");
    assert!(chain.contains("injected status polling failure"), "{chain}");
}

#[test]
fn cancellation_reaps_the_compiler_and_finishes_both_stream_logs() {
    let dir = Dir::new("compiler-cancel-logs");
    let cancel = AtomicBool::new(false);
    let compiler = shell_compiler(dir.path(), &cancel);
    let mut pid = None;
    let result = compiler.run(
        &script("printf 'compiler error detail\\n' >&2; echo $$; kill -STOP $$"),
        |line| {
            pid = line.parse::<i32>().ok().and_then(Pid::from_raw);
            cancel.store(true, Ordering::Relaxed);
        },
    );
    assert!(result.unwrap_err().is::<Cancelled>());
    assert_reaped(pid.expect("the compiler reported its PID before cancellation"));
    let log = std::fs::read_to_string(compiler_log(dir.path()).current()).unwrap();
    assert!(log.contains("compiler error detail"), "{log}");
    assert!(
        log.contains(&pid.unwrap().as_raw_nonzero().to_string()),
        "{log}"
    );
}
