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

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use launcher::local_worlds::world::{NATURAL_TERRAIN_REQUIRES_BDS, World};

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
    /// The largest radius a player may be granted, in chunks; `None` allows the client's maximum.
    pub view_distance: Option<u32>,
    /// No start has applied the world's options yet, so creation-only ones still apply.
    pub options_unapplied: bool,
    /// The signed-in host's XUID that BDS writes as its operator, as a vanilla world owner is. A
    /// [`HostGrant::AtLaunch`] runner gets the host resolved first; a [`HostGrant::WhenReady`] one
    /// gets the host known at launch, `None` while it still loads.
    pub host_xuid: Option<String>,
}

/// When a runner's servers apply the host's operator grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostGrant {
    /// The server makes nobody an operator, so its starts never wait for the host.
    None,
    /// The operator list is fixed at launch, so a start waits out a loading host before launching.
    AtLaunch,
    /// The server launches with the host known then, and takes [`Instance::grant_host`] after it
    /// is ready and before its world is reported running.
    WhenReady,
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
    /// Whether the server has exited with success, so it saved its world on the way out.
    fn exited_cleanly(&self) -> bool {
        false
    }
    /// Runs one server console command; unsupported unless the server takes commands.
    fn console(&self, _command: &str) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    /// Makes `xuid` the running server's only operator, or nobody, replacing its launch grant.
    /// Returns once the server acknowledges it, or cancellation or failure ends the wait. Servers of
    /// runners other than [`HostGrant::WhenReady`] cannot change their grant and return an error.
    fn grant_host(&self, xuid: Option<&str>, cancel: &Cancel) -> Result<(), WorldError>;
}

/// Launches local servers; `start` returns once the server accepts connections.
pub(crate) trait Runner: Send + Sync {
    fn start(&self, spec: &StartSpec, cancel: &Cancel) -> Result<Arc<dyn Instance>, WorldError>;
    /// When this runner's next start applies the host's operator grant.
    fn host_grant(&self) -> HostGrant;
    /// Readies a server ahead of a likely open, so the open skips the process's startup work.
    fn prepare(&self) {}
    /// Ends a readied server at once, as the client exits; a lock held by the exiting thread
    /// leaves it to the exit sweep.
    fn discard_standby(&self) {}
}

/// Hosts each Dragonfly world in `bedrock-local-server`, which prints `ready` once listening and
/// reads `pause`, `resume` and `stop` lines; stdin EOF also stops it, so it cannot outlive us.
/// Started with `-standby`, it waits for a `start <JSON args>` line.
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
        let world = &spec.world;
        if !world.generator.supports_backend(bridge::Backend::Dragonfly) {
            return Err(WorldError::Invalid(NATURAL_TERRAIN_REQUIRES_BDS.to_owned()));
        }
        let address = loopback(free_loopback_port()?);
        let mut args: Vec<OsString> = vec!["-dir".into(), spec.dir.clone().into()];
        for (flag, value) in [
            ("-addr", address.clone()),
            ("-name", world.name.clone()),
            ("-game-mode", wire_name(world.game_mode)),
            ("-difficulty", wire_name(world.difficulty)),
            ("-generator", wire_name(world.generator)),
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
                    None,
                )?
            }
        };
        let process: Arc<dyn Instance> = await_ready(spawned, self.start_timeout, cancel)?;
        Ok(process)
    }

    /// Dragonfly has no operator list for the host.
    fn host_grant(&self) -> HostGrant {
        HostGrant::None
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
            None,
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

/// How a launched child ended, as its [`ExitHook`] learns it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Ending {
    /// It exited successfully.
    Clean,
    /// Its start was cancelled before it was ready, and the kill that followed ended it.
    Cancelled,
    /// It could not be started.
    Unstarted,
    /// Anything else: an unsuccessful exit or crash, also during a stop, a start timeout, or a kill
    /// after a stop or exit sweep ran out of time.
    Failed,
}

/// Runs once with how a launched child ended: after it has been reaped and before its exit is
/// reported, or before the launch returns when it could not be started.
pub(crate) type ExitHook = Box<dyn FnOnce(Ending) + Send>;

/// A child that announces readiness on stdout and reads the `stop` line protocol on stdin.
pub(crate) struct Launch {
    pub address: String,
    pub timeout: Duration,
    /// False when the server has no faithful pause; pausing is then a no-op.
    pub can_pause: bool,
    pub ready: fn(&str) -> bool,
    /// Prefixes the server's log lines.
    pub label: String,
    /// Learns how the child ended. It must not wait for anything that waits for this child's exit.
    pub on_exit: Option<ExitHook>,
}

/// A launched server process.
pub(crate) struct Process {
    /// Tracked so the client's exit sweep ends it even when no destructor runs.
    child: Arc<Spawned>,
    stdin: Mutex<Option<ChildStdin>>,
    /// Serializes acknowledged commands while output readers can independently deliver a reply.
    console: Mutex<()>,
    output_ack: Arc<OutputAck>,
    /// Set at launch, or when a standby server is given its world.
    address: OnceLock<String>,
    can_pause: bool,
    exited: Receiver<()>,
    status: Arc<Mutex<Option<ExitStatus>>>,
    stop_requested: AtomicBool,
    /// Set before a cancelled start kills the child, so its ending reads as cancelled.
    start_cancelled: Arc<AtomicBool>,
}

/// One pending console response; registration always precedes writing the command.
#[derive(Default)]
struct OutputAck(Mutex<Option<(&'static str, Sender<()>)>>);

impl OutputAck {
    /// Delivers an exact response after stripping the server's optional log prefix.
    fn observe(&self, line: &str) {
        let line = line.trim();
        let message = if line.starts_with('[') {
            line.split_once("] ").map_or(line, |(_, message)| message)
        } else {
            line
        };
        if let Some((expected, sender)) = lock(&self.0).as_ref()
            && message == *expected
        {
            let _ = sender.try_send(());
        }
    }
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
        spec.on_exit,
    )?;
    await_ready(spawned, spec.timeout, cancel)
}

/// A started child and the signal of its ready line.
struct Spawn {
    process: Arc<Process>,
    ready: Receiver<()>,
}

/// Starts `command` as a tracked child whose output is logged under `label`; `on_exit` runs once
/// on every path, as [`ExitHook`] describes.
fn spawn(
    mut command: Command,
    address: Option<String>,
    can_pause: bool,
    is_ready: fn(&str) -> bool,
    label: String,
    on_exit: Option<ExitHook>,
) -> Result<Spawn, WorldError> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let child = match children::spawn_saving(&mut command, SAVE_BUDGET) {
        Ok(child) => child,
        Err(error) => {
            if let Some(hook) = on_exit {
                hook(Ending::Unstarted);
            }
            return Err(failed(format!("start local server: {error}")));
        }
    };
    tracing::info!(label, pid = child.id(), "local server spawned");
    let (stdin, stdout, stderr) = (child.take_stdin(), child.take_stdout(), child.take_stderr());
    let child = Arc::new(child);
    let status = Arc::new(Mutex::new(None));
    let start_cancelled = Arc::new(AtomicBool::new(false));
    let (ready_sender, ready) = crossbeam_channel::bounded(1);
    let (exit_sender, exited) = crossbeam_channel::bounded::<()>(0);
    let output_ack = Arc::new(OutputAck::default());
    let mut readers = Vec::new();
    if let Some(stdout) = stdout {
        let label = label.clone();
        let mut timing = super::startup_timing::StartupTiming::new(started, child.id());
        let output_ack = Arc::clone(&output_ack);
        readers.push(thread::spawn(move || {
            for_each_line(stdout, |line| {
                tracing::debug!("{label}: {line}");
                output_ack.observe(line);
                timing.observe(&label, line);
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
        let start_cancelled = Arc::clone(&start_cancelled);
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
            if let Some(hook) = on_exit {
                let exit = *lock(&status);
                hook(ending(exit, start_cancelled.load(Ordering::SeqCst)));
            }
            // Disconnects only after the hook, or while unwinding from a panicking one.
            drop(exit_sender);
        });
    }
    let process = Arc::new(Process {
        child,
        stdin: Mutex::new(stdin),
        console: Mutex::new(()),
        output_ack,
        address: address.map_or_else(OnceLock::new, OnceLock::from),
        can_pause,
        exited,
        status,
        stop_requested: AtomicBool::new(false),
        start_cancelled,
    });
    Ok(Spawn { process, ready })
}

/// Classifies a reaped child's `exit`; `cancelled` is set when a cancelled start killed it. A
/// cancelled child that ended any other way, such as a crash, is a failure.
fn ending(exit: Option<ExitStatus>, cancelled: bool) -> Ending {
    match exit {
        Some(exit) if exit.success() => Ending::Clean,
        Some(exit) if cancelled && killed(exit) => Ending::Cancelled,
        _ => Ending::Failed,
    }
}

/// Whether `exit` is the result of this process's kill.
#[cfg(unix)]
fn killed(exit: ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    exit.signal() == Some(libc::SIGKILL)
}

/// Whether `exit` is the result of this process's kill; Windows reports a killed child only as
/// an unsuccessful exit.
#[cfg(not(unix))]
fn killed(exit: ExitStatus) -> bool {
    !exit.success()
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
            recv(cancelled) -> _ => {
                process.start_cancelled.store(true, Ordering::SeqCst);
                break WorldError::Cancelled;
            }
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
    /// Sends a command and waits for its response, process exit, cancellation or the caller's budget.
    pub(crate) fn send_confirmed(
        &self,
        line: &str,
        response: &'static str,
        timeout: Duration,
        cancel: &Cancel,
    ) -> Result<(), WorldError> {
        let _console = lock(&self.console);
        if cancel.is_cancelled() {
            return Err(WorldError::Cancelled);
        }
        let (sender, replied) = crossbeam_channel::bounded(1);
        *lock(&self.output_ack.0) = Some((response, sender));
        let result = self
            .send(line)
            .map_err(|error| failed(format!("send local server command: {error}")))
            .and_then(|()| {
                crossbeam_channel::select! {
                    recv(replied) -> _ => Ok(()),
                    recv(self.exited) -> _ => Err(failed("local server exited before confirming its command")),
                    recv(cancel.receiver()) -> _ => Err(WorldError::Cancelled),
                    recv(crossbeam_channel::after(timeout)) -> _ => Err(failed("local server did not confirm its command in time")),
                }
            });
        lock(&self.output_ack.0).take();
        if cancel.is_cancelled() {
            return Err(WorldError::Cancelled);
        }
        result
    }

    /// Writes `line` and a newline to the server's console input; succeeding means only that the
    /// pipe took it, not that the server has read or applied it.
    pub(crate) fn send(&self, line: &str) -> io::Result<()> {
        if line.contains(['\n', '\r']) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
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

    fn exited_cleanly(&self) -> bool {
        self.has_exited() && lock(&self.status).is_some_and(|status| status.success())
    }

    /// A Dragonfly server has no operator list to change.
    fn grant_host(&self, _xuid: Option<&str>, _cancel: &Cancel) -> Result<(), WorldError> {
        Err(failed("this local server grants no operator"))
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
