//! Every helper process the client spawns stays tracked until reaped, so each exit path (normal
//! exit, the shutdown watchdog, a panic, SIGINT/SIGTERM) can end it by its own handle.

use std::{
    io,
    process::{Child, ChildStdout, Command, ExitStatus},
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
pub(crate) const EXIT_GRACE: Duration = Duration::from_millis(1_000);

static CHILDREN: Children = Children::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StopOutcome {
    /// Exited during the graceful step (stdin EOF, or already gone).
    Exited,
    Terminated,
    Killed,
    /// Survived SIGKILL's bounded wait; it stays tracked.
    Unreaped,
}

type Entry = Arc<Mutex<Child>>;

/// A registry of unreaped children.
pub(crate) struct Children {
    live: Mutex<Vec<Entry>>,
}

/// One tracked child. Dropping it unreaped leaves the child to the registry's exit sweep.
#[derive(Debug)]
pub(crate) struct Spawned(Entry);

impl Children {
    pub(crate) const fn new() -> Self {
        Self {
            live: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn track(&self, child: Child) -> Spawned {
        let entry = Arc::new(Mutex::new(child));
        let mut live = lock(&self.live);
        live.retain(|entry| running(&mut lock(entry)));
        live.push(Arc::clone(&entry));
        Spawned(entry)
    }

    /// Ends every tracked child: close stdin and wait `graceful`, then SIGTERM, then SIGKILL.
    pub(crate) fn stop_all(&self, graceful: Duration) {
        let entries = lock(&self.live).clone();
        escalate(&entries, graceful);
        lock(&self.live).retain(|entry| running(&mut lock(entry)));
    }

    /// Counts the children still registered by the Unix process-lifecycle tests.
    #[cfg(all(test, unix))]
    fn tracked(&self) -> usize {
        lock(&self.live).len()
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
pub(crate) fn spawn(command: &mut Command) -> io::Result<Spawned> {
    command.env(PARENT_ENV, std::process::id().to_string());
    command.spawn().map(Spawned::from)
}

/// Ends every child this process spawned; for exit paths.
pub(crate) fn stop_all(graceful: Duration) {
    CHILDREN.stop_all(graceful);
}

/// Stops every child when dropped, including while unwinding out of `run`.
pub(crate) struct StopOnDrop;

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        stop_all(EXIT_GRACE);
    }
}

/// Stops every child on a main-thread panic (the process is going down) and on SIGINT/SIGTERM,
/// which then exits with 130.
pub(crate) fn install_exit_hooks() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        if thread::current().name() == Some("main") {
            stop_all(Duration::ZERO);
        }
    }));
    let installed = ctrlc::try_set_handler(|| {
        stop_all(EXIT_GRACE);
        std::process::exit(130);
    });
    if let Err(error) = installed {
        bevy::log::warn!("child processes will not be stopped on SIGINT/SIGTERM: {error}");
    }
}

impl Spawned {
    #[cfg(test)]
    pub(crate) fn id(&self) -> u32 {
        lock(&self.0).id()
    }

    pub(crate) fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        lock(&self.0).try_wait()
    }

    pub(crate) fn take_stdout(&self) -> Option<ChildStdout> {
        lock(&self.0).stdout.take()
    }

    /// Sends SIGKILL without waiting; a no-op once reaped.
    pub(crate) fn kill(&self) {
        let _ = lock(&self.0).kill();
    }

    /// Waits for exit by polling, so an exit sweep can still lock this child meanwhile.
    pub(crate) fn wait(&self) -> Option<ExitStatus> {
        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => thread::sleep(POLL),
                Err(_) => return None,
            }
        }
    }

    pub(crate) fn stop(&self, graceful: Duration) -> StopOutcome {
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
