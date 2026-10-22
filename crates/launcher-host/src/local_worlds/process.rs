use std::{
    ffi::OsString,
    io::{self, BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{ChildStdin, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
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
/// How long a server may take to save and exit after its stop request; BDS saves the world
/// before exiting. Exit sweeps wait this long before signalling a server.
pub(crate) const SAVE_BUDGET: Duration = Duration::from_secs(30);
/// How often an exited server's status is collected once its output closes.
const EXIT_POLL: Duration = Duration::from_millis(10);

/// The world a runner must host.
pub(crate) struct StartSpec {
    pub world: World,
    pub dir: PathBuf,
    /// Chunks; `None` takes the backend's default.
    pub view_distance: Option<u32>,
    /// No start has applied the world's options yet, so creation-only ones still apply.
    pub options_unapplied: bool,
}

/// One running local server.
pub(crate) trait Instance: Send + Sync {
    /// The loopback game address.
    fn address(&self) -> &str;
    fn set_paused(&self, paused: bool) -> io::Result<()>;
    /// Asks the server to save and stop without waiting.
    fn request_stop(&self);
    /// Stops gracefully, killing the server once `timeout` passes, and joins backend cleanup
    /// before returning even when the process already exited or stopping reports an error.
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
    /// Readies a server ahead of a likely open, so the open skips the process's startup work.
    fn prepare(&self) {}
    /// Ends a readied server at once, as the client exits; a lock held by the exiting thread
    /// leaves it to the exit sweep.
    fn discard_standby(&self) {}
}

/// Hosts each Dragonfly world in `bedrock-local-server`, which prints `ready` once listening and
/// reads `pause`, `resume` and `stop` lines; stdin EOF also stops it, so it cannot outlive us.
/// Started with `-standby`, it warms its world generation and waits for a `start <JSON args>` line.
pub(crate) struct ProcessRunner {
    pub binary: PathBuf,
    /// Added to the inherited environment.
    pub env: Vec<(OsString, OsString)>,
    pub start_timeout: Duration,
    /// A standby server the next start takes over.
    standby: Mutex<Option<Spawn>>,
}

impl ProcessRunner {
    pub(crate) fn new(binary: PathBuf) -> Self {
        Self {
            binary,
            env: Vec::new(),
            start_timeout: DEFAULT_START_TIMEOUT,
            standby: Mutex::new(None),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.binary);
        command.envs(self.env.iter().map(|(key, value)| (key, value)));
        command
    }

    /// Hands `args` to a live standby server; `None` when there is none or it cannot take them.
    fn take_standby(&self, args: &[OsString], address: &str) -> Option<Spawn> {
        let standby = lock(&self.standby).take()?;
        let args: Option<Vec<&str>> = args.iter().map(|arg| arg.to_str()).collect();
        let line = serde_json::to_string(&args?).ok()?;
        if standby.process.has_exited() || standby.process.send(&format!("start {line}")).is_err() {
            standby.process.kill();
            return None;
        }
        let _ = standby.process.address.set(address.to_owned());
        Some(standby)
    }
}

/// The line `bedrock-local-server` prints once it accepts connections.
fn ready_line(line: &str) -> bool {
    line.trim() == "ready"
}

impl Runner for ProcessRunner {
    fn start(&self, spec: &StartSpec, cancel: &Cancel) -> Result<Arc<dyn Instance>, WorldError> {
        let address = loopback(free_loopback_port()?);
        let world = &spec.world;
        let mut args: Vec<OsString> = vec!["-dir".into(), spec.dir.clone().into()];
        for (flag, value) in [
            ("-addr", address.clone()),
            ("-name", world.name.clone()),
            ("-game-mode", wire_name(world.game_mode)),
            ("-difficulty", wire_name(world.difficulty)),
            ("-generator", wire_name(world.generator)),
            ("-seed", world.seed.to_string()),
        ] {
            args.extend([flag.into(), value.into()]);
        }
        let spawned = match self.take_standby(&args, &address) {
            Some(standby) => standby,
            None => {
                let mut command = self.command();
                command.args(&args);
                spawn(
                    command,
                    Some(address),
                    true,
                    ready_line,
                    format!("local-server {}", world.id),
                )?
            }
        };
        let process: Arc<dyn Instance> = await_ready(spawned, self.start_timeout, cancel)?;
        Ok(process)
    }

    fn prepare(&self) {
        let mut standby = lock(&self.standby);
        if standby
            .as_ref()
            .is_some_and(|standby| !standby.process.has_exited())
        {
            return;
        }
        let mut command = self.command();
        command.arg("-standby");
        match spawn(
            command,
            None,
            true,
            ready_line,
            "local-server standby".to_owned(),
        ) {
            Ok(spawned) => *standby = Some(spawned),
            Err(error) => tracing::warn!("prepare a standby local server: {error}"),
        }
    }

    fn discard_standby(&self) {
        let standby = match self.standby.try_lock() {
            Ok(mut standby) => standby.take(),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner().take(),
            Err(std::sync::TryLockError::WouldBlock) => None,
        };
        if let Some(standby) = standby {
            // A standby holds no world, so nothing needs saving.
            standby.process.kill();
        }
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
    /// Set at launch, or when a standby server is given its world.
    address: OnceLock<String>,
    can_pause: bool,
    exited: Receiver<()>,
    status: Arc<Mutex<Option<ExitStatus>>>,
    stop_requested: AtomicBool,
}

/// Starts `command` as a tracked child and waits for its ready line, killing it on exit, timeout
/// or cancellation.
pub(crate) fn launch(
    command: Command,
    spec: Launch,
    cancel: &Cancel,
) -> Result<Arc<Process>, WorldError> {
    let spawned = spawn(
        command,
        Some(spec.address),
        spec.can_pause,
        spec.ready,
        spec.label,
    )?;
    await_ready(spawned, spec.timeout, cancel)
}

/// A started child and the signal of its ready line.
struct Spawn {
    process: Arc<Process>,
    ready: Receiver<()>,
}

/// Starts `command` as a tracked child whose output is logged under `label`.
fn spawn(
    mut command: Command,
    address: Option<String>,
    can_pause: bool,
    is_ready: fn(&str) -> bool,
    label: String,
) -> Result<Spawn, WorldError> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = children::spawn_saving(&mut command, SAVE_BUDGET)
        .map_err(|error| failed(format!("start local server: {error}")))?;
    let (stdin, stdout, stderr) = (child.take_stdin(), child.take_stdout(), child.take_stderr());
    let child = Arc::new(child);
    let status = Arc::new(Mutex::new(None));
    let (ready_sender, ready) = crossbeam_channel::bounded(1);
    let (exit_sender, exited) = crossbeam_channel::bounded::<()>(0);
    let mut readers = Vec::new();
    if let Some(stdout) = stdout {
        let label = label.clone();
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
        let label = label.clone();
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
        address: address.map_or_else(OnceLock::new, OnceLock::from),
        can_pause,
        exited,
        status,
        stop_requested: AtomicBool::new(false),
    });
    Ok(Spawn { process, ready })
}

/// Waits for `spawned`'s ready line, killing it on exit, timeout or cancellation.
fn await_ready(
    spawned: Spawn,
    timeout: Duration,
    cancel: &Cancel,
) -> Result<Arc<Process>, WorldError> {
    let Spawn { process, ready } = spawned;
    let deadline = Instant::now() + timeout;
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
        self.address.get().map_or("", String::as_str)
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
