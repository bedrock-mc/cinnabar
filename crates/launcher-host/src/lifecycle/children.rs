//! Every helper process the client spawns stays tracked until reaped, so each exit path (normal
//! exit, the shutdown watchdog, a panic, SIGINT/SIGTERM) can end it by its own handle.

use std::{
    io,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
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

type Entry = Arc<Mutex<Child>>;
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
        let entry = Arc::new(Mutex::new(child));
        let mut live = lock(&self.live);
        live.entries.retain(|entry| running(&mut lock(entry)));
        if live.shutting_down {
            escalate(std::slice::from_ref(&entry), Duration::ZERO);
        }
        if running(&mut lock(&entry)) {
            live.entries.push(Arc::clone(&entry));
        }
        Spawned(entry)
    }

    /// Spawns and registers under the same lock that revokes shutdown authority.
    fn spawn(&self, command: &mut Command) -> io::Result<Spawned> {
        let mut live = lock(&self.live);
        if live.shutting_down {
            return Err(io::Error::other("client is shutting down"));
        }
        let entry = Arc::new(Mutex::new(command.spawn()?));
        live.entries.retain(|entry| running(&mut lock(entry)));
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

    /// Ends every tracked child: run the exit hooks, close stdin and wait `graceful`, then
    /// SIGTERM, then SIGKILL.
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
        escalate(&live.entries, graceful);
        live.entries.retain(|entry| running(&mut lock(entry)));
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
    CHILDREN.spawn(command)
}

/// Ends every child this process spawned; for exit paths.
pub fn stop_all(graceful: Duration) {
    CHILDREN.stop_all(graceful);
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
    let installed = ctrlc::try_set_handler(move || {
        stop_all(EXIT_GRACE);
        flush_logs();
        std::process::exit(130);
    });
    if let Err(error) = installed {
        tracing::warn!("child processes will not be stopped on SIGINT/SIGTERM: {error}");
    }
}

impl Spawned {
    /// Returns the tracked child PID for process-lifetime fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn id(&self) -> u32 {
        lock(&self.0).id()
    }

    pub fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        lock(&self.0).try_wait()
    }

    pub fn take_stdout(&self) -> Option<ChildStdout> {
        lock(&self.0).stdout.take()
    }

    pub fn take_stdin(&self) -> Option<ChildStdin> {
        lock(&self.0).stdin.take()
    }

    pub fn take_stderr(&self) -> Option<ChildStderr> {
        lock(&self.0).stderr.take()
    }

    /// Sends SIGKILL without waiting; a no-op once reaped.
    pub fn kill(&self) {
        let _ = lock(&self.0).kill();
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

fn escalate(entries: &[Entry], graceful: Duration) -> Vec<StopOutcome> {
    let mut outcomes = vec![StopOutcome::Unreaped; entries.len()];
    for entry in entries {
        drop(lock(entry).stdin.take());
    }
    let steps = [
        (StopOutcome::Exited, graceful),
        (StopOutcome::Terminated, TERM_WAIT),
        (StopOutcome::Killed, KILL_WAIT),
    ];
    for (step, (outcome, wait)) in steps.into_iter().enumerate() {
        if step > 0 {
            for (entry, done) in entries.iter().zip(&outcomes) {
                if *done == StopOutcome::Unreaped {
                    signal(&mut lock(entry), outcome);
                }
            }
        }
        let deadline = Instant::now() + wait;
        loop {
            for (entry, done) in entries.iter().zip(outcomes.iter_mut()) {
                if *done == StopOutcome::Unreaped && !running(&mut lock(entry)) {
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
/// have been reaped and reused.
fn signal(child: &mut Child, outcome: StopOutcome) {
    if !running(child) {
        return;
    }
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
fn running(child: &mut Child) -> bool {
    matches!(child.try_wait(), Ok(None))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
