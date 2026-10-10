//! The `serve` session over real pipes: the runtime binary loads an artifact, answers each
//! callback with its result, and exits with the code that the end of the session calls for.

use crate::common;

use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use common::{
    edit_manifest, interact, looping_register_dir, p, probe_dir, probe_dir_with, server_items, tell,
};
use experience_runtime::limits::{
    CALLBACK_FUEL, MAX_FRAME_BYTES, MAX_MANIFEST_BYTES, MAX_REASON_BYTES,
};
use experience_runtime::load::{engine, load};
use experience_runtime::protocol::{
    Op, Outcome, PROTOCOL_VERSION, Request, Response, read_frame, write_frame,
};
use experience_runtime::serve::{EXIT_LOAD_FAILED, EXIT_OK, EXIT_PROTOCOL};
use wasmtime::Trap;

/// How long a test waits for a frame or an exit before it gives up on the runtime.
const PATIENCE: Duration = Duration::from_secs(60);

/// A runtime process serving one session over its stdin and stdout. Dropping it kills the
/// process if it is still running.
struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    /// Every frame the runtime writes, then the end of its stdout (`None`) or a read error.
    frames: Receiver<io::Result<Option<Response>>>,
    stderr: Option<JoinHandle<String>>,
}

impl Session {
    /// Starts `experience-runtime serve` with `flags`.
    fn start(flags: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_experience-runtime"))
            .arg("serve")
            .args(flags)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("starting the runtime");
        let mut stdout = child.stdout.take().expect("piped stdout");
        let (sender, frames) = mpsc::channel();
        thread::spawn(move || {
            loop {
                let frame = read_frame(&mut stdout);
                let more = matches!(frame, Ok(Some(_)));
                if sender.send(frame).is_err() || !more {
                    break;
                }
            }
        });
        let mut stderr = child.stderr.take().expect("piped stderr");
        let stderr = thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).expect("reading stderr");
            text
        });
        Self {
            stdin: child.stdin.take(),
            child,
            frames,
            stderr: Some(stderr),
        }
    }

    fn stdin(&mut self) -> &mut ChildStdin {
        self.stdin.as_mut().expect("stdin is open")
    }

    fn send(&mut self, request: &Request) {
        write_frame(self.stdin(), request).expect("writing a frame");
    }

    /// Writes `body` as one frame, whatever it holds.
    fn send_raw(&mut self, body: &[u8]) {
        let length = u32::try_from(body.len()).unwrap().to_le_bytes();
        let stdin = self.stdin();
        stdin.write_all(&length).unwrap();
        stdin.write_all(body).unwrap();
        stdin.flush().unwrap();
    }

    /// Ends the runtime's stdin.
    fn close(&mut self) {
        drop(self.stdin.take());
    }

    fn receive(&self) -> Response {
        match self.frames.recv_timeout(PATIENCE) {
            Ok(Ok(Some(response))) => response,
            other => panic!("expected a frame, got {other:?}"),
        }
    }

    /// Loads `dir` and returns the answer.
    fn load(&mut self, dir: &Path) -> Response {
        self.send(&load_request(dir));
        self.receive()
    }

    /// Loads `dir`, which must succeed.
    fn load_ok(&mut self, dir: &Path) {
        let response = self.load(dir);
        assert!(matches!(response, Response::Loaded { .. }), "{response:?}");
    }

    /// Waits for the runtime to exit on its own, which must happen without another frame.
    /// Returns the exit code and everything the runtime logged.
    fn finish(&mut self) -> (i32, String) {
        let deadline = Instant::now() + PATIENCE;
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("polling the runtime") {
                break status;
            }
            assert!(Instant::now() < deadline, "the runtime did not exit");
            test_time::idle();
        };
        let stderr = self.stderr.take().expect("finished once");
        let stderr = stderr.join().expect("the stderr reader");
        match self.frames.recv_timeout(PATIENCE) {
            Ok(Ok(None)) => {}
            other => panic!("expected the end of stdout, got {other:?}\nstderr:\n{stderr}"),
        }
        (status.code().expect("an exit code"), stderr)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A failed test may leave the runtime running.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The `load` request for `dir`, with the server items an adapter lists for the probe.
fn load_request(dir: &Path) -> Request {
    Request::Load {
        dir: dir.to_str().expect("a UTF-8 path").to_owned(),
        items: server_items(),
    }
}

/// `request`, a callback, renumbered to `seq`.
fn numbered(mut request: Request, seq: u64) -> Request {
    let Request::Callback { seq: number, .. } = &mut request else {
        unreachable!("a callback request");
    };
    *number = seq;
    request
}

/// The session reports what loading the artifact yields, answers a callback, and ends on
/// `shutdown` while stdin is still open.
#[test]
fn load_then_callback_round_trip() {
    let dir = probe_dir();
    let (engine, _ticker) = engine().unwrap();
    let loaded = load(&engine, dir.path()).unwrap();
    let mut session = Session::start(&[]);
    assert_eq!(
        session.load(dir.path()),
        Response::Loaded {
            protocol: PROTOCOL_VERSION,
            id: loaded.manifest.id,
            version: loaded.manifest.version,
            blocks: loaded.blocks,
            items: loaded.items,
            focus: loaded.focus,
        }
    );
    session.send(&interact(0));
    assert_eq!(
        session.receive(),
        Response::Result {
            seq: 1,
            outcome: Outcome::Committed {
                ops: vec![
                    Op::SetBlockData {
                        pos: p(0),
                        data: Some("01000000".to_owned()),
                    },
                    tell("count 1"),
                ],
            },
        }
    );
    session.send(&Request::Shutdown {});
    assert_eq!(session.finish().0, EXIT_OK);
}

/// Loads `dir` in a fresh session, which must answer `load_failed` and then exit on its own with
/// the load-failed code. Returns the reason.
fn load_failure(dir: &Path) -> String {
    let mut session = Session::start(&[]);
    let reason = match session.load(dir) {
        Response::LoadFailed { reason } => reason,
        other => panic!("{other:?}"),
    };
    assert_eq!(session.finish().0, EXIT_LOAD_FAILED);
    reason
}

/// The runtime answers a failed load with the reason, which names the directory, and exits on
/// its own.
#[test]
fn load_failure_exits_1() {
    let empty = tempfile::tempdir().unwrap();
    let reason = load_failure(empty.path());
    let path = empty.path().display().to_string();
    assert!(
        reason.contains(&path),
        "the reason does not name {path}: {reason}"
    );
}

/// A load failure is answered with a reason cut to `MAX_REASON_BYTES`, however much the error
/// echoes: here an `api` that the manifest limit admits, and one longer than a frame, which the
/// limit refuses.
#[test]
fn load_failure_reason_is_bounded() {
    for len in [MAX_MANIFEST_BYTES / 2, MAX_FRAME_BYTES + 1] {
        let dir = probe_dir_with(|dir| {
            edit_manifest(dir, |manifest| {
                manifest.insert("api".to_owned(), "9".repeat(len).into());
            });
        });
        let reason = load_failure(dir.path());
        assert!(
            reason.len() <= MAX_REASON_BYTES,
            "an api of {len} bytes gave a reason of {} bytes",
            reason.len()
        );
        let path = dir.path().display().to_string();
        assert!(
            reason.contains(&path),
            "the reason does not name {path}: {reason}"
        );
    }
}

/// A trap in `register` is reported as its context and root cause. The wasm backtrace, which
/// grows with the guest's stack and its names, is left out.
#[test]
fn register_trap_reason_has_no_backtrace() {
    let dir = looping_register_dir();
    let reason = load_failure(dir.path());
    let ends_with_trap = [Trap::OutOfFuel, Trap::Interrupt]
        .iter()
        .any(|trap| reason.ends_with(&trap.to_string()));
    assert!(ends_with_trap && !reason.contains('\n'), "{reason}");
}

/// A frame that cannot be written ends the session with the protocol code, because the
/// load-failed code promises that `load_failed` was answered.
#[test]
fn unwritable_output_exits_2() {
    let empty = tempfile::tempdir().unwrap();
    #[cfg(windows)]
    let output = tempfile::NamedTempFile::new().unwrap();
    #[cfg(windows)]
    let stdout = Stdio::from(std::fs::File::open(output.path()).unwrap());
    #[cfg(unix)]
    let (stdout, peer) = {
        use std::{net::Shutdown, os::fd::OwnedFd, os::unix::net::UnixStream};

        let (output, peer) = UnixStream::pair().unwrap();
        // A valid socket returns a write failure instead of stdout's ignored EBADF.
        // Shutdown state survives inherited descriptor copies.
        output.shutdown(Shutdown::Write).unwrap();
        (Stdio::from(OwnedFd::from(output)), peer)
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_experience-runtime"))
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(stdout)
        .stderr(Stdio::piped())
        .spawn()
        .expect("starting the runtime");
    let mut stdin = child.stdin.take().expect("piped stdin");
    write_frame(&mut stdin, &load_request(empty.path())).unwrap();
    drop(stdin);
    let result = child.wait_with_output().expect("waiting for the runtime");
    #[cfg(unix)]
    drop(peer);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(result.status.code(), Some(EXIT_PROTOCOL), "{stderr}");
    assert!(stderr.contains("writing a frame"), "{stderr}");
}

/// A first frame other than `load`, or a second `load`, ends the session unanswered.
#[test]
fn wrong_frame_exits_2() {
    for first in [interact(0), Request::Shutdown {}] {
        let mut session = Session::start(&[]);
        session.send(&first);
        assert_eq!(session.finish().0, EXIT_PROTOCOL, "first frame {first:?}");
    }
    let dir = probe_dir();
    let mut session = Session::start(&[]);
    session.load_ok(dir.path());
    session.send(&load_request(dir.path()));
    assert_eq!(session.finish().0, EXIT_PROTOCOL);
}

/// A frame that is not a request, or one cut off by the end of stdin, ends the session
/// unanswered, before or after loading.
#[test]
fn undecodable_frame_exits_2() {
    let mut session = Session::start(&[]);
    session.send_raw(b"not json");
    assert_eq!(session.finish().0, EXIT_PROTOCOL);

    let dir = probe_dir();
    let mut session = Session::start(&[]);
    session.load_ok(dir.path());
    session.send_raw(br#"{"type":"reload"}"#);
    assert_eq!(session.finish().0, EXIT_PROTOCOL);

    let mut session = Session::start(&[]);
    session.load_ok(dir.path());
    // A length that promises more bytes than arrive.
    session.stdin().write_all(&[10, 0, 0, 0, b'{']).unwrap();
    session.close();
    assert_eq!(session.finish().0, EXIT_PROTOCOL);
}

/// The end of stdin between frames ends the session cleanly, before or after loading.
#[test]
fn eof_exits_0() {
    let mut session = Session::start(&[]);
    session.close();
    assert_eq!(session.finish().0, EXIT_OK);

    let dir = probe_dir();
    let mut session = Session::start(&[]);
    session.load_ok(dir.path());
    session.close();
    assert_eq!(session.finish().0, EXIT_OK);
}

/// Each result carries its callback's `seq`, in the order the callbacks came, whether it
/// committed, was rejected by the guest, or failed.
#[test]
fn seq_is_echoed() {
    let dir = probe_dir();
    let mut session = Session::start(&[]);
    session.load_ok(dir.path());
    let callbacks = [(u64::MAX, 0), (0, 10), (42, 1)];
    for (seq, x) in callbacks {
        session.send(&numbered(interact(x), seq));
    }
    let results: Vec<(u64, Outcome)> = callbacks
        .iter()
        .map(|_| match session.receive() {
            Response::Result { seq, outcome } => (seq, outcome),
            other => panic!("{other:?}"),
        })
        .collect();
    assert!(
        matches!(
            results[..],
            [
                (u64::MAX, Outcome::Committed { .. }),
                (0, Outcome::Rejected { .. }),
                (42, Outcome::Failed { .. }),
            ]
        ),
        "{results:?}"
    );
    session.close();
    assert_eq!(session.finish().0, EXIT_OK);
}

/// With `--report-fuel`, each callback logs the fuel it consumed. The endless loop consumes all
/// of it.
#[test]
fn report_fuel_logs_each_callback() {
    let dir = probe_dir();
    let mut session = Session::start(&["--report-fuel"]);
    let Response::Loaded { id, .. } = session.load(dir.path()) else {
        panic!("the probe did not load");
    };
    for x in [0, 2] {
        session.send(&interact(x));
        session.receive();
    }
    session.close();
    let (code, stderr) = session.finish();
    assert_eq!(code, EXIT_OK);
    let prefix = format!("fuel {id} interact ");
    let fuel: Vec<u64> = stderr
        .lines()
        .filter_map(|line| line.strip_prefix(&prefix))
        .map(|consumed| consumed.parse().expect("fuel is a number"))
        .collect();
    assert!(
        matches!(fuel[..], [some, all] if 0 < some && some < CALLBACK_FUEL && all == CALLBACK_FUEL),
        "{stderr}"
    );
}
