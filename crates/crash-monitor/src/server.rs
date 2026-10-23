//! The monitor's side of the `minidumper` connection: it writes requested dumps and records
//! everything the client tells it. Only messages that start with the launch token count, since
//! `minidumper` cannot say which connection sent one.

use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{Receiver, SyncSender},
    },
    time::Duration,
};

use crash_report::report::Kind;
use launcher_host::lifecycle::crash::heartbeat::Heartbeat;
#[cfg(windows)]
use launcher_host::lifecycle::crash::protocol::DumpContext;
use launcher_host::lifecycle::crash::protocol::{self, Hello, Panic, State, Token};
use minidumper::{LoopAction, MinidumpBinary, ServerHandler};

/// How long the server may take to read what the client sent before it went away.
const DRAIN: Duration = Duration::from_secs(2);

/// A dump the client asked for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Dump {
    /// The written minidump, or `None` when writing it failed.
    pub path: Option<PathBuf>,
    /// Unix milliseconds when it was requested.
    pub at_ms: u64,
}

/// A panic whose dump arrived while the client lived on, with the state it had then.
#[derive(Clone, Debug)]
pub(crate) struct DumpedPanic {
    pub panic: Panic,
    pub dump: Dump,
    pub state: State,
}

/// The error `main` failed with, and whether the game had started.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Failure {
    /// [`Kind::Startup`] or [`Kind::Runtime`].
    pub kind: Kind,
    pub error: String,
}

/// What the client told the monitor before it went away.
#[derive(Clone, Debug, Default)]
pub(crate) struct Observed {
    pub hello: Option<Hello>,
    pub state: State,
    /// The panic the next dump belongs to; one without its dump when that never came.
    pub panic: Option<Panic>,
    /// Dumped panics waiting for their folders.
    pub dumped_panics: Vec<DumpedPanic>,
    /// A native fault's dump.
    pub dump: Option<Dump>,
    /// The code of an announced exit.
    pub exit: Option<i32>,
    pub failure: Option<Failure>,
    /// The client announced the dump being written now.
    pub dump_requested: bool,
    /// The faulting context of the next dump, for full-memory dumps.
    #[cfg(windows)]
    pub dump_context: Option<DumpContext>,
}

/// Something the monitor's main thread acts on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Event {
    /// A panic's dump arrived; its folder can be written now.
    PanicDumped,
    /// The client closed the monitor's stdin: it is gone.
    ClientGone,
}

/// Whether the client is gone, shared with the server so it stops after draining connections.
#[derive(Debug, Default)]
pub(crate) struct Liveness {
    gone: AtomicBool,
}

impl Liveness {
    /// Lets the server read queued messages after the client exits, then requests shutdown if
    /// its connections do not close within the drain budget.
    pub(crate) fn drain(&self, finished: &Receiver<()>, shutdown: &AtomicBool) {
        self.gone.store(true, Ordering::SeqCst);
        // No accepted connections can also mean a connection is still queued for acceptance.
        if finished.recv_timeout(DRAIN).is_err() {
            shutdown.store(true, Ordering::Relaxed);
        }
    }

    /// Stops after the final connection closes, once the client's exit has been recorded.
    fn closed(&self, open: usize) -> LoopAction {
        if open == 0 && self.gone.load(Ordering::SeqCst) {
            LoopAction::Exit
        } else {
            LoopAction::Continue
        }
    }
}

/// Records messages into shared [`Observed`] and writes dumps beside the crash folders.
pub(crate) struct Handler {
    observed: Arc<Mutex<Observed>>,
    crash_dir: PathBuf,
    now_ms: fn() -> u64,
    token: Token,
    events: SyncSender<Event>,
    liveness: Arc<Liveness>,
    /// Numbers pending dump files, so a dump never overwrites one not yet moved.
    dumps: AtomicU32,
    /// The pending file of the dump being written.
    writing: Mutex<Option<PathBuf>>,
    /// Receipts shared with the client, so a crash never waits on a socket read.
    heartbeat: Option<Heartbeat>,
}

impl Handler {
    pub(crate) fn new(
        observed: Arc<Mutex<Observed>>,
        crash_dir: PathBuf,
        now_ms: fn() -> u64,
        token: Token,
        events: SyncSender<Event>,
        liveness: Arc<Liveness>,
    ) -> Self {
        Self {
            observed,
            crash_dir,
            now_ms,
            token,
            events,
            liveness,
            dumps: AtomicU32::new(0),
            writing: Mutex::new(None),
            heartbeat: None,
        }
    }

    /// Enables receipt of authenticated dump announcements through the client's shared region.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn with_dump_receipts(mut self, heartbeat: Option<Heartbeat>) -> Self {
        self.heartbeat = heartbeat;
        self
    }

    fn observed(&self) -> MutexGuard<'_, Observed> {
        self.observed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A file a dump is written to before its crash folder exists; folder listing and pruning
/// ignore it.
fn pending_dump(crash_dir: &Path, number: u32) -> PathBuf {
    crash_dir.join(format!(".pending-{}-{number}.dmp", std::process::id()))
}

impl ServerHandler for Handler {
    fn create_minidump_file(&self) -> Result<(File, PathBuf), std::io::Error> {
        std::fs::create_dir_all(&self.crash_dir)?;
        let path = pending_dump(&self.crash_dir, self.dumps.fetch_add(1, Ordering::Relaxed));
        let file = crate::dump::create(&path)?;
        *self.writing.lock().unwrap_or_else(PoisonError::into_inner) = Some(path.clone());
        Ok((file, path))
    }

    fn on_minidump_created(&self, result: Result<MinidumpBinary, minidumper::Error>) -> LoopAction {
        let at_ms = (self.now_ms)();
        let pending = self
            .writing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let mut observed = self.observed();
        if !std::mem::take(&mut observed.dump_requested) {
            drop(observed);
            drop(result);
            tracing::warn!("ignored a dump the client did not request");
            if let Some(pending) = pending {
                let _ = std::fs::remove_file(pending);
            }
            return LoopAction::Continue;
        }
        #[cfg(windows)]
        let full_memory = observed
            .dump_context
            .take()
            .filter(|_| observed.hello.as_ref().is_some_and(|hello| hello.full_dump));
        drop(observed);
        let path = match result {
            Ok(binary) => {
                #[cfg(windows)]
                let binary = match full_memory {
                    Some(context) => {
                        let mut binary = binary;
                        crate::dump::rewrite_with_full_memory(&mut binary.file, context);
                        binary
                    }
                    None => binary,
                };
                drop(binary.file);
                tracing::info!("minidump written");
                Some(binary.path)
            }
            Err(error) => {
                tracing::error!("minidump failed: {error}");
                if let Some(pending) = pending {
                    let _ = std::fs::remove_file(pending);
                }
                None
            }
        };
        let dump = Dump { path, at_ms };
        let mut observed = self.observed();
        match observed.panic.take() {
            Some(panic) => {
                let state = observed.state.clone();
                if observed.dumped_panics.len() == crash_report::folder::MAX_REPORTS {
                    let discarded = observed.dumped_panics.remove(0);
                    if let Some(path) = discarded.dump.path {
                        let _ = std::fs::remove_file(path);
                    }
                    tracing::warn!(
                        "pending panic reports reached their retention bound; discarded the oldest dump"
                    );
                }
                observed
                    .dumped_panics
                    .push(DumpedPanic { panic, dump, state });
                drop(observed);
                let _ = self.events.try_send(Event::PanicDumped);
            }
            None => observed.dump = Some(dump),
        }
        LoopAction::Continue
    }

    fn on_message(&self, kind: u32, buffer: Vec<u8>) {
        let Some(payload) = protocol::open(&self.token, &buffer) else {
            tracing::warn!("ignored message {kind} without this launch's token");
            return;
        };
        let mut observed = self.observed();
        match kind {
            protocol::HELLO => match serde_json::from_slice(payload) {
                Ok(hello) => {
                    observed.hello.get_or_insert(hello);
                }
                Err(error) => tracing::warn!("unreadable client hello: {error}"),
            },
            protocol::STATE => match serde_json::from_slice(payload) {
                Ok(state) => observed.state = state,
                Err(error) => tracing::warn!("unreadable client state: {error}"),
            },
            protocol::PANIC => match serde_json::from_slice(payload) {
                Ok(panic) => observed.panic = Some(panic),
                Err(error) => tracing::warn!("unreadable panic: {error}"),
            },
            protocol::EXIT => {
                if let Ok(code) = <[u8; 4]>::try_from(payload) {
                    observed.exit.get_or_insert(i32::from_le_bytes(code));
                }
            }
            protocol::STARTUP_FAILURE | protocol::RUNTIME_FAILURE => {
                observed.failure = Some(Failure {
                    kind: if kind == protocol::STARTUP_FAILURE {
                        Kind::Startup
                    } else {
                        Kind::Runtime
                    },
                    error: String::from_utf8_lossy(payload).into_owned(),
                });
            }
            protocol::DUMP_REQUEST => {
                observed.dump_requested = true;
                if let Some(heartbeat) = &self.heartbeat
                    && let Ok(request) = <[u8; 8]>::try_from(payload)
                {
                    heartbeat
                        .region()
                        .acknowledge_dump(u64::from_le_bytes(request));
                }
                #[cfg(windows)]
                {
                    observed.dump_context = DumpContext::from_bytes(payload);
                }
            }
            other => tracing::warn!("unknown client message {other}"),
        }
    }

    fn on_client_connected(&self, open: usize) -> LoopAction {
        if open > 1 {
            tracing::warn!("another process connected to the crash monitor");
        }
        LoopAction::Continue
    }

    /// A dump ends its connection while the client may live on and connect again, so the
    /// server stops only once the client is gone and every connection is closed.
    fn on_client_disconnected(&self, open: usize) -> LoopAction {
        self.liveness.closed(open)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crash_report::report::Identity;

    use super::*;

    const TOKEN: Token = [3; protocol::TOKEN_BYTES];

    fn handler() -> (Handler, Arc<Mutex<Observed>>, mpsc::Receiver<Event>) {
        let observed = Arc::new(Mutex::new(Observed::default()));
        let (events, received) = mpsc::sync_channel(1);
        let handler = Handler::new(
            Arc::clone(&observed),
            std::env::temp_dir(),
            || 5_000,
            TOKEN,
            events,
            Arc::default(),
        );
        (handler, observed, received)
    }

    fn signed(token: &Token, payload: &[u8]) -> Vec<u8> {
        [token.as_slice(), payload].concat()
    }

    fn hello() -> Vec<u8> {
        serde_json::to_vec(&Hello {
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
        })
        .unwrap()
    }

    fn panic() -> Vec<u8> {
        serde_json::to_vec(&Panic {
            message: "forged".into(),
            thread: crash_report::report::Thread::default(),
            backtrace: String::new(),
            uptime_ms: 0,
        })
        .unwrap()
    }

    #[test]
    #[cfg(unix)]
    fn minidumps_are_private_from_their_first_write() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let (mut handler, _, _) = handler();
        handler.crash_dir = directory.path().to_owned();
        let (file, _) = handler.create_minidump_file().unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o077, 0);
    }

    #[test]
    fn messages_without_the_launch_token_are_ignored() {
        let (handler, observed, _) = handler();
        let forged = [9; protocol::TOKEN_BYTES];
        handler.on_message(protocol::HELLO, signed(&forged, &hello()));
        handler.on_message(protocol::HELLO, hello());
        handler.on_message(protocol::PANIC, signed(&forged, &panic()));
        handler.on_message(protocol::STARTUP_FAILURE, b"forged".to_vec());
        handler.on_message(protocol::DUMP_REQUEST, Vec::new());
        {
            let observed = observed.lock().unwrap();
            assert!(observed.hello.is_none());
            assert!(observed.panic.is_none());
            assert!(observed.failure.is_none());
            assert!(!observed.dump_requested);
        }
        handler.on_message(protocol::HELLO, signed(&TOKEN, &hello()));
        handler.on_message(
            protocol::RUNTIME_FAILURE,
            signed(&TOKEN, b"network session stopped"),
        );
        let observed = observed.lock().unwrap();
        assert!(observed.hello.is_some());
        assert_eq!(
            observed.failure,
            Some(Failure {
                kind: Kind::Runtime,
                error: "network session stopped".into(),
            })
        );
    }

    #[test]
    fn only_requested_dumps_count() {
        let (handler, observed, received) = handler();
        let error = || Err(minidumper::Error::ProtocolError("test dump"));
        handler.on_minidump_created(error());
        assert!(observed.lock().unwrap().dump.is_none());
        handler.on_message(protocol::DUMP_REQUEST, signed(&TOKEN, &[]));
        handler.on_minidump_created(error());
        assert_eq!(
            observed.lock().unwrap().dump,
            Some(Dump {
                path: None,
                at_ms: 5_000,
            })
        );
        assert!(received.try_recv().is_err());
        handler.on_message(protocol::PANIC, signed(&TOKEN, &panic()));
        handler.on_message(protocol::DUMP_REQUEST, signed(&TOKEN, &[]));
        handler.on_minidump_created(error());
        assert_eq!(received.try_recv(), Ok(Event::PanicDumped));
        let observed = observed.lock().unwrap();
        assert_eq!(observed.dumped_panics.len(), 1);
        assert!(observed.panic.is_none());
    }

    /// Only this launch's authenticated request may publish its exact shared receipt.
    #[test]
    fn dump_receipts_require_the_launch_token() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("heartbeat");
        let client = Heartbeat::create(&path).unwrap();
        let (handler, observed, _) = handler();
        let handler = handler.with_dump_receipts(Some(Heartbeat::open(&path).unwrap()));
        let request = 42_u64.to_le_bytes();
        handler.on_message(protocol::DUMP_REQUEST, request.to_vec());
        handler.on_message(
            protocol::DUMP_REQUEST,
            signed(&[9; protocol::TOKEN_BYTES], &request),
        );
        assert!(!client.region().dump_acknowledged(42));
        assert!(!observed.lock().unwrap().dump_requested);
        handler.on_message(protocol::PANIC, signed(&TOKEN, &panic()));
        handler.on_message(protocol::DUMP_REQUEST, signed(&TOKEN, &request));
        assert!(client.region().dump_acknowledged(42));
        assert!(!client.region().dump_acknowledged(43));
        let observed = observed.lock().unwrap();
        assert!(observed.dump_requested);
        assert!(observed.panic.is_some());
    }

    #[test]
    fn panic_bursts_bound_pending_dumps_and_coalesce_wakeups() {
        let (handler, observed, received) = handler();
        for _ in 0..128 {
            handler.on_message(protocol::PANIC, signed(&TOKEN, &panic()));
            handler.on_message(protocol::DUMP_REQUEST, signed(&TOKEN, &[]));
            handler.on_minidump_created(Err(minidumper::Error::ProtocolError("fixture dump")));
        }
        assert_eq!(
            observed.lock().unwrap().dumped_panics.len(),
            crash_report::folder::MAX_REPORTS
        );
        assert_eq!(received.try_iter().count(), 1);
    }

    #[test]
    fn the_server_stops_once_the_client_is_gone_and_its_connections_closed() {
        let liveness = Liveness::default();
        // A dump closes the connection of a client that lives on.
        assert!(liveness.closed(0) == LoopAction::Continue);
        liveness.gone.store(true, Ordering::SeqCst);
        assert!(liveness.closed(1) == LoopAction::Continue);
        assert!(liveness.closed(0) == LoopAction::Exit);
    }

    /// The client can send its entire session and exit before the server accepts its socket.
    #[test]
    #[cfg(not(target_os = "macos"))]
    fn failures_queued_before_accept_are_drained_after_client_exit() {
        for (message, kind) in [
            (protocol::STARTUP_FAILURE, Kind::Startup),
            (protocol::RUNTIME_FAILURE, Kind::Runtime),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let socket = directory.path().join("socket");
            let mut server = minidumper::Server::with_name(protocol::socket_name(&socket)).unwrap();
            let client = minidumper::Client::with_name(protocol::socket_name(&socket)).unwrap();
            for (message, payload) in [
                (protocol::HELLO, hello()),
                (message, b"fixture failure".to_vec()),
                (protocol::EXIT, 1_i32.to_le_bytes().to_vec()),
            ] {
                client
                    .send_message(message, signed(&TOKEN, &payload))
                    .unwrap();
            }
            drop(client);

            let (handler, observed, _received) = handler();
            let liveness = Arc::clone(&handler.liveness);
            let shutdown = Arc::new(AtomicBool::new(false));
            let (finished, server_finished) = mpsc::channel();
            let serving = {
                let liveness = Arc::clone(&liveness);
                let shutdown = Arc::clone(&shutdown);
                std::thread::spawn(move || {
                    test_time::eventually("the client's exit", || {
                        liveness.gone.load(Ordering::SeqCst)
                    });
                    server.run(Box::new(handler), &shutdown, None).unwrap();
                    finished.send(()).unwrap();
                })
            };
            liveness.drain(&server_finished, &shutdown);
            serving.join().unwrap();
            let observed = observed.lock().unwrap();
            assert!(observed.hello.is_some());
            assert_eq!(observed.exit, Some(1));
            assert_eq!(
                observed.failure,
                Some(Failure {
                    kind,
                    error: "fixture failure".into(),
                })
            );
        }
    }
}
