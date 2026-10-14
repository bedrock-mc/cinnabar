//! The monitor's side of the `minidumper` connection: it writes requested dumps and records
//! everything the client tells it.

use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use launcher_host::lifecycle::crash::protocol::{self, DumpContext, Hello, Panic, State};
use minidumper::{LoopAction, MinidumpBinary, ServerHandler};

/// A dump the client asked for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Dump {
    /// The written minidump, or `None` when writing it failed.
    pub path: Option<PathBuf>,
    /// Unix milliseconds when it was requested.
    pub at_ms: u64,
}

/// What the client told the monitor before it went away.
#[derive(Clone, Debug, Default)]
pub(crate) struct Observed {
    pub hello: Option<Hello>,
    pub state: State,
    pub panic: Option<Panic>,
    pub dump: Option<Dump>,
    /// The code of an announced, deliberate exit.
    pub exit: Option<i32>,
    pub startup_failure: Option<String>,
    /// The faulting context of the next dump, for full-memory dumps on Windows.
    pub dump_context: Option<DumpContext>,
}

/// Records messages into shared [`Observed`] and writes dumps beside the crash folders.
pub(crate) struct Handler {
    observed: Arc<Mutex<Observed>>,
    crash_dir: PathBuf,
    now_ms: fn() -> u64,
}

impl Handler {
    pub(crate) fn new(
        observed: Arc<Mutex<Observed>>,
        crash_dir: PathBuf,
        now_ms: fn() -> u64,
    ) -> Self {
        Self {
            observed,
            crash_dir,
            now_ms,
        }
    }

    fn observed(&self) -> MutexGuard<'_, Observed> {
        self.observed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The file a dump is written to before its crash folder exists; folder listing and pruning
/// ignore it.
pub(crate) fn pending_dump(crash_dir: &Path) -> PathBuf {
    crash_dir.join(format!(".pending-{}.dmp", std::process::id()))
}

impl ServerHandler for Handler {
    fn create_minidump_file(&self) -> Result<(File, PathBuf), std::io::Error> {
        std::fs::create_dir_all(&self.crash_dir)?;
        let path = pending_dump(&self.crash_dir);
        Ok((File::create(&path)?, path))
    }

    fn on_minidump_created(&self, result: Result<MinidumpBinary, minidumper::Error>) -> LoopAction {
        let at_ms = (self.now_ms)();
        let path = match result {
            Ok(mut binary) => {
                #[cfg(windows)]
                {
                    let observed = self.observed();
                    let full = observed.hello.as_ref().is_some_and(|hello| hello.full_dump);
                    if let (true, Some(context)) = (full, observed.dump_context) {
                        drop(observed);
                        crate::dump::rewrite_with_full_memory(&mut binary.file, context);
                    }
                }
                drop(binary.file);
                tracing::info!("minidump written");
                Some(binary.path)
            }
            Err(error) => {
                tracing::error!("minidump failed: {error}");
                let _ = std::fs::remove_file(pending_dump(&self.crash_dir));
                None
            }
        };
        self.observed().dump = Some(Dump { path, at_ms });
        LoopAction::Continue
    }

    fn on_message(&self, kind: u32, buffer: Vec<u8>) {
        let mut observed = self.observed();
        match kind {
            protocol::HELLO => match serde_json::from_slice(&buffer) {
                Ok(hello) => observed.hello = Some(hello),
                Err(error) => tracing::warn!("unreadable client hello: {error}"),
            },
            protocol::STATE => match serde_json::from_slice(&buffer) {
                Ok(state) => observed.state = state,
                Err(error) => tracing::warn!("unreadable client state: {error}"),
            },
            protocol::PANIC => match serde_json::from_slice(&buffer) {
                Ok(panic) => observed.panic = Some(panic),
                Err(error) => tracing::warn!("unreadable panic: {error}"),
            },
            protocol::EXIT => {
                if let Ok(code) = <[u8; 4]>::try_from(buffer.as_slice()) {
                    observed.exit.get_or_insert(i32::from_le_bytes(code));
                }
            }
            protocol::STARTUP_FAILURE => {
                observed.startup_failure = Some(String::from_utf8_lossy(&buffer).into_owned());
            }
            protocol::DUMP_CONTEXT => observed.dump_context = DumpContext::from_bytes(&buffer),
            other => tracing::warn!("unknown client message {other}"),
        }
    }

    /// The client is the only one: once it is gone, so is everything it could still say.
    fn on_client_disconnected(&self, clients: usize) -> LoopAction {
        if clients == 0 {
            LoopAction::Exit
        } else {
            LoopAction::Continue
        }
    }
}
