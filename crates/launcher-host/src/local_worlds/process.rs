use std::{
    ffi::OsString,
    io::{self, BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{ChildStdin, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, TryRecvError};
use launcher::local_worlds::world::World;

use crate::lifecycle::children::{self, Spawned};

use super::{
    Cancel, lock,
    ports::{free_loopback_port, loopback},
    world::{WorldError, failed, wire_name},
};

const DEFAULT_START_TIMEOUT: Duration = Duration::from_secs(30);
/// How often an exited server's status is collected once its output closes.
const EXIT_POLL: Duration = Duration::from_millis(10);

/// The world a runner must host.
pub(crate) struct StartSpec {
    pub world: World,
    pub dir: PathBuf,
    /// Chunks; `None` takes the backend's default.
    pub view_distance: Option<u32>,
}

/// One running local server.
pub(crate) trait Instance: Send + Sync {
    /// The loopback game address.
    fn address(&self) -> &str;
    fn set_paused(&self, paused: bool) -> io::Result<()>;
    /// Asks the server to save and stop without waiting.
    fn request_stop(&self);
    /// Stops gracefully, killing the server once `timeout` passes.
    fn stop(&self, timeout: Duration) -> Result<(), String>;
    /// Disconnects once the server has exited.
    fn exited(&self) -> Receiver<()>;
    /// False when [`Self::set_paused`] has no effect.
    fn can_pause(&self) -> bool {
        true
    }
    /// The player limit, the host included, when the runner sets one.
    fn max_players(&self) -> Option<u32> {
        None
    }
    /// The loopback LAN discovery endpoint, when the server advertises there.
    fn lan_address(&self) -> Option<&str> {
        None
    }
}

/// Launches local servers; `start` returns once the server accepts connections.
pub(crate) trait Runner: Send + Sync {
    fn start(&self, spec: &StartSpec, cancel: &Cancel) -> Result<Arc<dyn Instance>, WorldError>;
}

/// Hosts each Dragonfly world in `bedrock-local-server`, which prints `ready` once listening and
/// reads `pause`, `resume` and `stop` lines; stdin EOF also stops it, so it cannot outlive us.
pub(crate) struct ProcessRunner {
    pub binary: PathBuf,
    /// Added to the inherited environment.
    pub env: Vec<(OsString, OsString)>,
    pub start_timeout: Duration,
}

impl ProcessRunner {
    pub(crate) fn new(binary: PathBuf) -> Self {
        Self {
            binary,
            env: Vec::new(),
            start_timeout: DEFAULT_START_TIMEOUT,
        }
    }
}

impl Runner for ProcessRunner {
    fn start(&self, spec: &StartSpec, cancel: &Cancel) -> Result<Arc<dyn Instance>, WorldError> {
        let address = loopback(free_loopback_port()?);
        let world = &spec.world;
        let mut command = Command::new(&self.binary);
        command
            .arg("-dir")
            .arg(&spec.dir)
            .args(["-addr", address.as_str(), "-name", world.name.as_str()])
            .args(["-game-mode", wire_name(world.game_mode).as_str()])
            .args(["-difficulty", wire_name(world.difficulty).as_str()])
            .args(["-generator", wire_name(world.generator).as_str()])
            .args(["-seed", world.seed.to_string().as_str()])
            .envs(self.env.iter().map(|(key, value)| (key, value)));
        let process: Arc<dyn Instance> = launch(
            command,
            Launch {
                address,
                timeout: self.start_timeout,
                can_pause: true,
                ready: |line| line.trim() == "ready",
                label: format!("local-server {}", world.id),
            },
            cancel,
        )?;
        Ok(process)
    }
}

/// A child that announces readiness on stdout and reads the `stop` line protocol on stdin.
pub(crate) struct Launch {
    pub address: String,
    pub timeout: Duration,
    /// False when the server has no faithful pause; pausing is then a no-op.
    pub can_pause: bool,
    pub ready: fn(&str) -> bool,
    /// Prefixes the server's log lines.
    pub label: String,
}

/// A launched server process.
pub(crate) struct Process {
    /// Tracked so the client's exit sweep ends it even when no destructor runs.
    child: Arc<Spawned>,
    stdin: Mutex<Option<ChildStdin>>,
    address: String,
    can_pause: bool,
    exited: Receiver<()>,
    status: Arc<Mutex<Option<ExitStatus>>>,
    stop_requested: AtomicBool,
}

/// Starts `command` as a tracked child and waits for its ready line, killing it on exit, timeout
/// or cancellation.
pub(crate) fn launch(
    mut command: Command,
    spec: Launch,
    cancel: &Cancel,
) -> Result<Arc<Process>, WorldError> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = children::spawn(&mut command)
        .map_err(|error| failed(format!("start local server: {error}")))?;
    let (stdin, stdout, stderr) = (child.take_stdin(), child.take_stdout(), child.take_stderr());
    let child = Arc::new(child);
    let status = Arc::new(Mutex::new(None));
    let (ready_sender, ready) = crossbeam_channel::bounded(1);
    let (exit_sender, exited) = crossbeam_channel::bounded::<()>(0);
    let mut readers = Vec::new();
    if let Some(stdout) = stdout {
        let (label, is_ready) = (spec.label.clone(), spec.ready);
        readers.push(thread::spawn(move || {
            for_each_line(stdout, |line| {
                tracing::debug!("{label}: {line}");
                if is_ready(line) {
                    let _ = ready_sender.try_send(());
                }
            });
        }));
    }
    if let Some(stderr) = stderr {
        let label = spec.label.clone();
        readers.push(thread::spawn(move || {
            for_each_line(stderr, |line| tracing::info!("{label}: {line}"));
        }));
    }
    {
        let (child, status) = (Arc::clone(&child), Arc::clone(&status));
        thread::spawn(move || {
            for reader in readers {
                let _ = reader.join();
            }
            loop {
                match child.try_wait() {
                    Ok(Some(exit)) => {
                        *lock(&status) = Some(exit);
                        break;
                    }
                    Ok(None) => thread::sleep(EXIT_POLL),
                    Err(_) => break,
                }
            }
            drop(exit_sender);
        });
    }
    let process = Arc::new(Process {
        child,
        stdin: Mutex::new(stdin),
        address: spec.address,
        can_pause: spec.can_pause,
        exited,
        status,
        stop_requested: AtomicBool::new(false),
    });
    let deadline = Instant::now() + spec.timeout;
    let (exit, cancelled) = (process.exited.clone(), cancel.receiver().clone());
    let mut announcing = true;
    let failure = loop {
        let ready = if announcing {
            ready.clone()
        } else {
            crossbeam_channel::never()
        };
        crossbeam_channel::select! {
            recv(ready) -> message => {
                if message.is_ok() {
                    return Ok(process);
                }
                announcing = false;
            }
            recv(exit) -> _ => {
                break failed(format!("local server exited during startup: {}", process.exit_text()));
            }
            recv(cancelled) -> _ => break WorldError::Cancelled,
            recv(crossbeam_channel::at(deadline)) -> _ => {
                break failed("local server did not become ready in time");
            }
        }
    };
    process.kill();
    let _ = process.exited.recv();
    Err(failure)
}

/// Calls `line` with each line of `reader`, without its line ending.
fn for_each_line(reader: impl Read, mut line: impl FnMut(&str)) {
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(_) => line(String::from_utf8_lossy(&buffer).trim_end_matches(['\n', '\r'])),
        }
    }
}

impl Process {
    fn send(&self, line: &str) -> io::Result<()> {
        let mut stdin = lock(&self.stdin);
        let pipe = stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("the server's stdin is closed"))?;
        pipe.write_all(format!("{line}\n").as_bytes())?;
        pipe.flush()
    }

    fn kill(&self) {
        self.child.kill();
    }

    fn has_exited(&self) -> bool {
        matches!(self.exited.try_recv(), Err(TryRecvError::Disconnected))
    }

    fn exit_text(&self) -> String {
        lock(&self.status).map_or_else(|| "unknown status".to_owned(), |exit| exit.to_string())
    }
}

impl Instance for Process {
    fn address(&self) -> &str {
        &self.address
    }

    fn set_paused(&self, paused: bool) -> io::Result<()> {
        if !self.can_pause {
            return Ok(());
        }
        self.send(if paused { "pause" } else { "resume" })
    }

    fn request_stop(&self) {
        if !self.stop_requested.swap(true, Ordering::SeqCst) {
            let _ = self.send("stop");
        }
    }

    fn stop(&self, timeout: Duration) -> Result<(), String> {
        if self.has_exited() {
            return Ok(());
        }
        self.request_stop();
        if self.exited.recv_timeout(timeout) != Err(RecvTimeoutError::Timeout) {
            return Ok(());
        }
        self.kill();
        let _ = self.exited.recv();
        Err("local server killed after shutdown timeout".to_owned())
    }

    fn exited(&self) -> Receiver<()> {
        self.exited.clone()
    }

    fn can_pause(&self) -> bool {
        self.can_pause
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
