//! Every helper process the client spawns stays tracked until reaped, so each exit path (normal
//! exit, the shutdown watchdog, a panic, SIGINT/SIGTERM) can end it by its own handle. iOS
//! cannot start processes, so there a core runs in this process and is tracked the same way.

use std::{
    io,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// Tells a spawned core which PID to outlive; bedrock-core's `lifeline.ParentEnv` reads it.
const PARENT_ENV: &str = "BEDROCK_CORE_PARENT_PID";
const POLL: Duration = Duration::from_millis(10);
/// Escalation after the graceful step: SIGTERM, then SIGKILL, each bounded.
const TERM_WAIT: Duration = Duration::from_millis(300);
const KILL_WAIT: Duration = Duration::from_millis(200);
/// Graceful step on exit paths without a deadline of their own; with the escalation this stays
/// inside the 2 s shutdown watchdog.
pub const EXIT_GRACE: Duration = Duration::from_millis(1_000);

static CHILDREN: Children = Children::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopOutcome {
    /// Exited during the graceful step (stdin EOF, or already gone).
    Exited,
    Terminated,
    Killed,
    /// Survived SIGKILL's bounded wait; it stays tracked.
    Unreaped,
}

/// A tracked worker: a child process, or on iOS a core running in this process.
#[derive(Debug)]
enum Process {
    Os(Child),
    #[cfg(target_os = "ios")]
    Embedded(embedded::Core),
}

impl Process {
    fn id(&self) -> u32 {
        match self {
            Self::Os(child) => child.id(),
            #[cfg(target_os = "ios")]
            Self::Embedded(core) => core.id(),
        }
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match self {
            Self::Os(child) => child.try_wait(),
            #[cfg(target_os = "ios")]
            Self::Embedded(core) => Ok(core.try_wait()),
        }
    }

    /// SIGKILL for a child; an in-process core can only be cancelled.
    fn kill(&mut self) {
        match self {
            Self::Os(child) => {
                let _ = child.kill();
            }
            #[cfg(target_os = "ios")]
            Self::Embedded(core) => core.cancel(),
        }
    }

    fn take_stdin(&mut self) -> Option<ChildStdin> {
        match self {
            Self::Os(child) => child.stdin.take(),
            #[cfg(target_os = "ios")]
            Self::Embedded(core) => core.stdin.take(),
        }
    }

    fn take_stdout(&mut self) -> Option<ChildStdout> {
        match self {
            Self::Os(child) => child.stdout.take(),
            #[cfg(target_os = "ios")]
            Self::Embedded(core) => core.stdout.take(),
        }
    }

    fn take_stderr(&mut self) -> Option<ChildStderr> {
        match self {
            Self::Os(child) => child.stderr.take(),
            #[cfg(target_os = "ios")]
            Self::Embedded(core) => core.stderr.take(),
        }
    }
}

/// A child and how long it may take to exit once asked to stop, before any signal reaches it.
#[derive(Debug)]
struct Tracked {
    child: Mutex<Process>,
    grace: Duration,
    /// Exit sweeps leave it running: it watches this process and ends itself afterwards.
    outlives: bool,
    /// Set once this process asks the running child to stop: stdin closed by a stop, a signal,
    /// or a kill. An exit after it was asked for, not the child's own.
    stop_requested: AtomicBool,
}

impl Tracked {
    fn new(child: Process, grace: Duration, outlives: bool) -> Self {
        Self {
            child: Mutex::new(child),
            grace,
            outlives,
            stop_requested: AtomicBool::new(false),
        }
    }

    /// Records a stop request when the child is still running; one that already exited ended
    /// on its own.
    fn request_stop(&self, child: &mut Process) {
        if running(child) {
            self.stop_requested.store(true, Ordering::Release);
        }
    }
}

type Entry = Arc<Tracked>;
type ExitHook = Arc<dyn Fn() + Send + Sync>;

/// A registry of unreaped children.
pub struct Children {
    live: Mutex<Registry>,
}

/// Registration and spawn authority share the shutdown lock.
struct Registry {
    entries: Vec<Entry>,
    shutting_down: bool,
    hooks: Vec<(u64, ExitHook)>,
    next_hook: u64,
}

/// Unregisters its exit hook when dropped.
pub struct ExitHookGuard<'a> {
    children: &'a Children,
    id: u64,
}

impl Drop for ExitHookGuard<'_> {
    fn drop(&mut self) {
        lock(&self.children.live)
            .hooks
            .retain(|(id, _)| *id != self.id);
    }
}

/// One tracked child. Dropping it unreaped leaves the child to the registry's exit sweep.
#[derive(Debug)]
pub struct Spawned(Entry);

impl Children {
    pub const fn new() -> Self {
        Self {
            live: Mutex::new(Registry {
                entries: Vec::new(),
                shutting_down: false,
                hooks: Vec::new(),
                next_hook: 0,
            }),
        }
    }

    pub fn track(&self, child: Child) -> Spawned {
        self.track_with(child, Duration::ZERO)
    }

    /// [`Self::track`] for a child that needs up to `grace` to exit after its stop request.
    fn track_with(&self, child: Child, grace: Duration) -> Spawned {
        let entry = Arc::new(Tracked::new(Process::Os(child), grace, false));
        let mut live = lock(&self.live);
        live.entries
            .retain(|entry| running(&mut lock(&entry.child)));
        if live.shutting_down {
            escalate(std::slice::from_ref(&entry), Duration::ZERO);
        }
        if running(&mut lock(&entry.child)) {
            live.entries.push(Arc::clone(&entry));
        }
        Spawned(entry)
    }

    /// Spawns and registers a child; see [`Self::start`].
    fn spawn(&self, command: &mut Command, grace: Duration, outlives: bool) -> io::Result<Spawned> {
        self.start(grace, outlives, || {
            #[cfg(unix)]
            seal_inherited_sockets();
            #[cfg(all(windows, not(debug_assertions)))]
            crate::desktop::windows::hide_console(command);
            command.spawn().map(Process::Os)
        })
    }

    /// Starts and registers a worker under the same lock that revokes shutdown authority; exit
    /// sweeps wait `grace` for it before signalling it, or skip an `outlives` worker entirely.
    fn start(
        &self,
        grace: Duration,
        outlives: bool,
        start: impl FnOnce() -> io::Result<Process>,
    ) -> io::Result<Spawned> {
        let mut live = lock(&self.live);
        if live.shutting_down {
            return Err(io::Error::other("client is shutting down"));
        }
        let entry = Arc::new(Tracked::new(start()?, grace, outlives));
        live.entries
            .retain(|entry| running(&mut lock(&entry.child)));
        live.entries.push(Arc::clone(&entry));
        Ok(Spawned(entry))
    }

    /// Runs `hook` once at the start of [`Self::stop_all`], until the guard drops, so a child can
    /// be asked to stop its own way (a server's save command) before the signals; registered
    /// during shutdown, it runs at once.
    pub fn on_exit(&self, hook: impl Fn() + Send + Sync + 'static) -> ExitHookGuard<'_> {
        let mut live = lock(&self.live);
        let id = live.next_hook;
        live.next_hook += 1;
        if live.shutting_down {
            drop(live);
            hook();
        } else {
            live.hooks.push((id, Arc::new(hook)));
        }
        ExitHookGuard { children: self, id }
    }

    /// Ends every tracked child except those that outlive this process: run the exit hooks,
    /// close stdin and wait `graceful`, then SIGTERM, then SIGKILL.
    pub fn stop_all(&self, graceful: Duration) {
        let hooks: Vec<ExitHook> = {
            let mut live = lock(&self.live);
            live.shutting_down = true;
            live.hooks.drain(..).map(|(_, hook)| hook).collect()
        };
        // Outside the lock: a hook may drop the last owner of another hook's guard.
        for hook in hooks {
            hook();
        }
        let mut live = lock(&self.live);
        let stopping: Vec<Entry> = live
            .entries
            .iter()
            .filter(|entry| !entry.outlives)
            .cloned()
            .collect();
        escalate(&stopping, graceful);
        live.entries
            .retain(|entry| running(&mut lock(&entry.child)));
    }

    /// Allows an installer handoff only after every owned worker has actually exited.
    fn handoff_after_stopping<T>(&self, handoff: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        self.stop_all(EXIT_GRACE);
        if lock(&self.live)
            .entries
            .iter()
            .any(|entry| !entry.outlives && running(&mut lock(&entry.child)))
        {
            return Err(io::Error::other("a client worker is still running"));
        }
        handoff()
    }

    /// Counts the children still registered by the Unix process-lifecycle tests.
    #[cfg(all(test, unix))]
    fn tracked(&self) -> usize {
        lock(&self.live).entries.len()
    }
}

impl Default for Children {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Child> for Spawned {
    fn from(child: Child) -> Self {
        CHILDREN.track(child)
    }
}

/// Spawns `command` as a tracked child that knows to exit with this process.
pub fn spawn(command: &mut Command) -> io::Result<Spawned> {
    command.env(PARENT_ENV, std::process::id().to_string());
    CHILDREN.spawn(command, Duration::ZERO, false)
}

/// [`spawn`] for a child that saves after its stop request: exit sweeps wait up to `save_budget`
/// for it to exit before signalling it.
pub fn spawn_saving(command: &mut Command, save_budget: Duration) -> io::Result<Spawned> {
    command.env(PARENT_ENV, std::process::id().to_string());
    CHILDREN.spawn(command, save_budget, false)
}

/// Spawns a tracked watcher that exit sweeps never stop, such as the crash monitor, which must
/// see this process end and then report it; the child must end itself once this process is gone.
pub fn spawn_outliving(command: &mut Command) -> io::Result<Spawned> {
    CHILDREN.spawn(command, Duration::ZERO, true)
}

/// Runs a core command in this process, since iOS cannot start processes: the linked core gets
/// the command's arguments and pipes for the requested streams. It is tracked like a child:
/// closing its stdin, then cancelling it, stand in for stdin EOF and SIGTERM.
#[cfg(target_os = "ios")]
pub fn spawn_embedded_core(
    command: &Command,
    stdin: bool,
    stdout: bool,
    stderr: bool,
) -> io::Result<Spawned> {
    CHILDREN.start(Duration::ZERO, false, || {
        embedded::Core::start(command, stdin, stdout, stderr).map(Process::Embedded)
    })
}

#[cfg(target_os = "ios")]
mod embedded;

/// Marks every open socket close-on-exec before a spawn, so no child holds a listener or
/// connection a C library opened without the flag (Tracy's listener would stall every capture).
#[cfg(unix)]
fn seal_inherited_sockets() {
    use rustix::process::{Resource, getrlimit};
    let limit = getrlimit(Resource::Nofile)
        .current
        .unwrap_or(1024)
        .min(4096);
    for raw in 3..limit as i32 {
        let _ = socket_seal::seal(raw);
    }
}

#[cfg(unix)]
mod socket_seal;

/// Ends every child this process spawned; for exit paths.
pub fn stop_all(graceful: Duration) {
    CHILDREN.stop_all(graceful);
}

/// Stops and reaps owned workers before starting a helper that will wait for the client's exit.
pub fn handoff_after_stopping<T>(handoff: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    CHILDREN.handoff_after_stopping(handoff)
}

/// Runs `hook` when this process's children are stopped on exit, until the guard drops.
pub fn on_exit(hook: impl Fn() + Send + Sync + 'static) -> ExitHookGuard<'static> {
    CHILDREN.on_exit(hook)
}

/// Stops every child when dropped, including while unwinding out of `run`.
pub struct StopOnDrop;

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        stop_all(EXIT_GRACE);
    }
}

/// Stops every child on a main-thread panic (the process is going down) and on SIGINT/SIGTERM,
/// which flushes the caller's logs and then exits with 130.
pub fn install_exit_hooks(flush_logs: fn()) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        if thread::current().name() == Some("main") {
            stop_all(Duration::ZERO);
        }
    }));
    let installed = ctrlc::set_handler(move || {
        crate::renderer_startup::termination::cancel_on_termination();
        stop_all(EXIT_GRACE);
        flush_logs();
        crate::lifecycle::crash::announce_exit(130);
        std::process::exit(130);
    });
    if let Err(error) = installed {
        tracing::warn!("child processes will not be stopped on SIGINT/SIGTERM: {error}");
    }
}

impl Spawned {
    /// The child's process id.
    pub fn id(&self) -> u32 {
        lock(&self.0.child).id()
    }

    /// Duplicates this child's Windows process handle without looking its PID up again.
    #[cfg(windows)]
    pub(crate) fn process_handle(&self) -> io::Result<std::os::windows::io::OwnedHandle> {
        use std::os::windows::io::AsHandle;
        let Process::Os(child) = &*lock(&self.0.child);
        child.as_handle().try_clone_to_owned()
    }

    pub fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        lock(&self.0.child).try_wait()
    }

    pub fn take_stdout(&self) -> Option<ChildStdout> {
        lock(&self.0.child).take_stdout()
    }

    pub fn take_stdin(&self) -> Option<ChildStdin> {
        lock(&self.0.child).take_stdin()
    }

    pub fn take_stderr(&self) -> Option<ChildStderr> {
        lock(&self.0.child).take_stderr()
    }

    /// Sends SIGKILL without waiting; a no-op once reaped.
    pub fn kill(&self) {
        let mut child = lock(&self.0.child);
        self.0.request_stop(&mut child);
        child.kill();
    }

    /// A handle a watcher thread uses to see this child's exit, without authority to stop it.
    pub fn observer(&self) -> ExitObserver {
        ExitObserver(Arc::clone(&self.0))
    }

    /// Waits for exit by polling, so an exit sweep can still lock this child meanwhile.
    pub fn wait(&self) -> Option<ExitStatus> {
        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => thread::sleep(POLL),
                Err(_) => return None,
            }
        }
    }

    pub fn stop(&self, graceful: Duration) -> StopOutcome {
        escalate(std::slice::from_ref(&self.0), graceful)[0]
    }
}

/// Observes one tracked child's exit from another thread.
#[derive(Debug)]
pub struct ExitObserver(Entry);

impl ExitObserver {
    pub fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        lock(&self.0.child).try_wait()
    }

    /// Whether this process asked the child to stop while it ran (a stop, an exit sweep or a
    /// kill), so its exit was requested rather than its own.
    pub fn stop_requested(&self) -> bool {
        self.0.stop_requested.load(Ordering::Acquire)
    }
}

fn escalate(entries: &[Entry], graceful: Duration) -> Vec<StopOutcome> {
    let mut outcomes = vec![StopOutcome::Unreaped; entries.len()];
    for entry in entries {
        let mut child = lock(&entry.child);
        entry.request_stop(&mut child);
        drop(child.take_stdin());
    }
    let steps = [
        (StopOutcome::Exited, graceful),
        (StopOutcome::Terminated, TERM_WAIT),
        (StopOutcome::Killed, KILL_WAIT),
    ];
    for (step, (outcome, wait)) in steps.into_iter().enumerate() {
        // Nothing is signalled before every child's own grace, so a saving server finishes; the
        // longest grace bounds the whole sweep.
        let wait = if step == 0 {
            entries
                .iter()
                .map(|entry| entry.grace)
                .fold(wait, Duration::max)
        } else {
            wait
        };
        if step > 0 {
            for (entry, done) in entries.iter().zip(&outcomes) {
                if *done == StopOutcome::Unreaped {
                    signal(&mut lock(&entry.child), outcome);
                }
            }
        }
        let deadline = Instant::now() + wait;
        loop {
            for (entry, done) in entries.iter().zip(outcomes.iter_mut()) {
                if *done == StopOutcome::Unreaped && !running(&mut lock(&entry.child)) {
                    *done = outcome;
                }
            }
            if !outcomes.contains(&StopOutcome::Unreaped) || Instant::now() >= deadline {
                break;
            }
            thread::sleep(POLL);
        }
        if !outcomes.contains(&StopOutcome::Unreaped) {
            break;
        }
    }
    outcomes
}

/// Signals a child the caller has just seen running under this same lock, so its PID cannot
/// have been reaped and reused. An in-process core has no signals and is cancelled instead.
fn signal(process: &mut Process, outcome: StopOutcome) {
    if !running(process) {
        return;
    }
    match process {
        Process::Os(child) => signal_child(child, outcome),
        #[cfg(target_os = "ios")]
        Process::Embedded(core) => core.cancel(),
    }
}

fn signal_child(child: &mut Child, outcome: StopOutcome) {
    #[cfg(unix)]
    if outcome == StopOutcome::Terminated {
        let pid = i32::try_from(child.id())
            .ok()
            .and_then(rustix::process::Pid::from_raw);
        if let Some(pid) = pid {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
        }
        return;
    }
    // Windows has no SIGTERM; its escalation kills at the terminate step.
    let _ = outcome;
    let _ = child.kill();
}

/// An inspection error means the child is not ours to signal any more.
fn running(child: &mut Process) -> bool {
    matches!(child.try_wait(), Ok(None))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod update_tests {
    use super::*;
    use std::{io::Read, process::Stdio};

    #[test]
    fn updater_handoff_observes_the_core_exit_after_graceful_stdin_close() {
        let children = Children::new();
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/Q", "/C", "set /p hold= & echo stopped & exit /b 0"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "read -r hold; echo stopped"]);
            command
        };
        command.stdin(Stdio::piped()).stdout(Stdio::piped());
        let core = children.spawn(&mut command, Duration::ZERO, false).unwrap();
        let mut output = core.take_stdout().unwrap();
        let mut handed_off = false;
        children
            .handoff_after_stopping(|| {
                assert!(core.try_wait()?.unwrap().success());
                let mut saved = String::new();
                output.read_to_string(&mut saved)?;
                assert!(
                    saved.contains("stopped"),
                    "the core must finish its EOF path"
                );
                handed_off = true;
                Ok(())
            })
            .unwrap();
        assert!(handed_off);
        assert!(children.spawn(&mut command, Duration::ZERO, false).is_err());
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{io::Read, process::Stdio};

    use super::*;

    fn sh(script: &str) -> Child {
        Command::new("sh")
            .args(["-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn holds_stdin() -> Child {
        sh("read -r hold")
    }

    fn ignores_stdin() -> Child {
        sh("exec sleep 30 </dev/null")
    }

    fn ignores_term() -> Child {
        sh("trap '' TERM; while :; do sleep 0.05; done </dev/null")
    }

    fn gone(pid: u32) -> bool {
        let pid = rustix::process::Pid::from_raw(pid as i32).unwrap();
        rustix::process::test_kill_process(pid).is_err()
    }

    const SOCKET_PROBE_FD: &str = "CINNABAR_SOCKET_PROBE_FD";
    // Distinct nonzero codes also reject a helper filter that accidentally runs no tests.
    const SOCKET_PROBE_CLOSED: i32 = 42;
    const SOCKET_PROBE_OPEN: i32 = 43;

    /// Reports descriptor validity in an executed child, without opening or redirecting it.
    #[test]
    fn socket_descriptor_probe() {
        let Some(fd) = std::env::var_os(SOCKET_PROBE_FD) else {
            return;
        };
        let fd: i32 = fd.to_str().unwrap().parse().unwrap();
        // SAFETY: F_GETFD takes a raw descriptor and permits invalid descriptors, returning
        // EBADF instead of requiring a valid Rust descriptor borrow.
        let flags = unsafe {
            assert!(libc::fcntl(libc::STDIN_FILENO, libc::F_GETFD) >= 0);
            libc::fcntl(fd, libc::F_GETFD)
        };
        let code = if flags >= 0 {
            SOCKET_PROBE_OPEN
        } else {
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
            SOCKET_PROBE_CLOSED
        };
        std::process::exit(code);
    }

    /// Runs the descriptor helper with usable standard input and explicit open/closed results.
    fn socket_probe(fd: i32) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "lifecycle::children::tests::socket_descriptor_probe",
                "--test-threads=1",
            ])
            .env(SOCKET_PROBE_FD, fd.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        command
    }

    // A socket a C library opened without close-on-exec (Tracy's listener) must not reach a child.
    #[test]
    fn spawned_children_do_not_inherit_foreign_sockets() {
        use rustix::io::{FdFlags, fcntl_dupfd_cloexec, fcntl_getfd, fcntl_setfd};
        use std::os::{
            fd::{AsRawFd, BorrowedFd},
            unix::process::CommandExt,
        };
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        // Exercise a descriptor above 9 even when this test runs alone.
        let socket = fcntl_dupfd_cloexec(&listener, 10).unwrap();
        fcntl_setfd(&socket, FdFlags::empty()).unwrap();
        let fd = socket.as_raw_fd();
        let mut control = socket_probe(fd);
        // SAFETY: only async-signal-safe fcntl runs after fork. The live socket owns the
        // descriptor, and changing its child copy cannot race with parent socket sealing.
        unsafe {
            control.pre_exec(move || {
                fcntl_setfd(BorrowedFd::borrow_raw(fd), FdFlags::empty()).map_err(io::Error::from)
            });
        }
        assert_eq!(
            control.status().unwrap().code(),
            Some(SOCKET_PROBE_OPEN),
            "the probe must detect the deliberately inherited socket"
        );
        let child = Children::new()
            .spawn(&mut socket_probe(fd), Duration::ZERO, false)
            .unwrap();
        assert!(fcntl_getfd(&socket).unwrap().contains(FdFlags::CLOEXEC));
        assert_eq!(
            child.wait().and_then(|status| status.code()),
            Some(SOCKET_PROBE_CLOSED),
            "child inherited descriptor {fd}"
        );
    }

    // Each exit path's sweep ends every child, escalating as far as each one needs.
    #[test]
    fn review_shutdown_also_stops_late_child_registration() {
        let children = Children::new();
        children.stop_all(Duration::ZERO);
        let child = children.track(ignores_stdin());
        let alive = matches!(child.try_wait(), Ok(None));
        child.stop(Duration::ZERO);
        assert!(!alive, "late registration escaped the exit sweep");
    }

    #[test]
    fn stop_all_escalates_until_every_child_is_reaped() {
        let children = Children::new();
        let graceful = children.track(holds_stdin());
        let terminated = children.track(ignores_stdin());
        let killed = children.track(ignores_term());
        let dropped = children.track(ignores_stdin());
        let dropped_pid = dropped.id();
        drop(dropped);

        children.stop_all(Duration::from_millis(100));

        assert_eq!(children.tracked(), 0);
        for child in [&graceful, &terminated, &killed] {
            assert!(matches!(child.try_wait(), Ok(Some(_))));
        }
        assert!(gone(dropped_pid));
    }

    #[test]
    fn stop_reports_the_step_that_ended_the_child() {
        let children = Children::new();
        let short = Duration::from_millis(50);
        assert_eq!(
            children.track(holds_stdin()).stop(short),
            StopOutcome::Exited
        );
        assert_eq!(
            children.track(ignores_stdin()).stop(short),
            StopOutcome::Terminated
        );
        assert_eq!(
            children.track(ignores_term()).stop(short),
            StopOutcome::Killed
        );
    }

    // A core being stopped on a reaper thread cannot outlive the process's exit sweep.
    #[test]
    fn a_detached_stop_cannot_outlive_the_exit_sweep() {
        let children = Children::new();
        let core = children.track(ignores_term());
        let pid = core.id();
        let reaper = thread::spawn(move || core.stop(Duration::from_secs(30)));
        thread::sleep(Duration::from_millis(50));

        let started = Instant::now();
        children.stop_all(Duration::ZERO);

        assert!(gone(pid));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(reaper.join().unwrap(), StopOutcome::Exited);
    }

    // A saving child is never signalled before its budget, so a slow save still completes.
    #[test]
    fn saving_children_exit_on_their_own_within_their_budget() {
        let children = Children::new();
        let saving = children.track_with(
            sh("trap '' TERM; sleep 0.5; exit 7"),
            Duration::from_secs(10),
        );
        let started = Instant::now();
        children.stop_all(Duration::ZERO);
        assert_eq!(
            saving.try_wait().unwrap().and_then(|status| status.code()),
            Some(7)
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn exit_hooks_run_once_before_the_sweep_until_dropped() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let children = Children::new();
        let runs = Arc::new(AtomicUsize::new(0));
        let count = |amount: usize| {
            let runs = Arc::clone(&runs);
            move || {
                runs.fetch_add(amount, Ordering::SeqCst);
            }
        };
        let kept = children.on_exit(count(1));
        drop(children.on_exit(count(10)));
        children.stop_all(Duration::ZERO);
        children.stop_all(Duration::ZERO);
        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "hooks run once and dropped ones never"
        );
        let late = children.on_exit(count(100));
        assert_eq!(
            runs.load(Ordering::SeqCst),
            101,
            "a hook registered during shutdown runs at once"
        );
        drop((kept, late));
    }

    #[test]
    fn spawned_children_learn_which_pid_to_outlive() {
        let child = spawn(
            Command::new("sh")
                .args(["-c", &format!("printf %s \"${PARENT_ENV}\"")])
                .stdin(Stdio::null())
                .stdout(Stdio::piped()),
        )
        .unwrap();
        let mut printed = String::new();
        child
            .take_stdout()
            .unwrap()
            .read_to_string(&mut printed)
            .unwrap();
        child.wait();
        assert_eq!(printed, std::process::id().to_string());
    }
}
