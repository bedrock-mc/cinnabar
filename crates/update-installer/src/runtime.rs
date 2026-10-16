//! The no-live-game gate: running installed processes hold it shared, and installation or
//! recovery needs it exclusively, so files are never replaced under a live game tree.

use std::{fs::File, io, path::Path, process::Command, time::Duration};

const RUNTIME_LOCK: &str = "runtime.lock";
/// Longest an installer waits for a just-closed game tree to finish exiting.
pub const QUIESCE_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest a starting game waits for an installation to release the gate.
pub const JOIN_TIMEOUT: Duration = Duration::from_secs(120);

/// One running process's share of the gate, held for its whole lifetime.
#[derive(Debug)]
pub struct Session {
    file: File,
}

impl Session {
    /// Joins without waiting; `WouldBlock` means an installation holds the gate.
    pub fn try_join(update_dir: &Path) -> io::Result<Self> {
        let file = open(update_dir)?;
        file.try_lock_shared()?;
        Ok(Self { file })
    }

    /// Joins once any installation finishes, bounded by `timeout` on the caller's clock.
    pub fn join(
        update_dir: &Path,
        timeout: Duration,
        elapsed: impl FnMut() -> Duration,
        pause: impl FnMut(),
    ) -> io::Result<Self> {
        wait(timeout, elapsed, pause, || Self::try_join(update_dir))
    }

    /// Lets `command`'s child hold this share until it exits too; no other child inherits it.
    pub fn share_with(&self, command: &mut Command) {
        #[cfg(unix)]
        {
            use std::os::{
                fd::{AsRawFd, BorrowedFd},
                unix::process::CommandExt,
            };
            let fd = self.file.as_raw_fd();
            // SAFETY: only async-signal-safe fcntl runs after fork, on a descriptor this session
            // keeps open for the parent's lifetime.
            unsafe {
                command.pre_exec(move || {
                    rustix::io::fcntl_setfd(
                        BorrowedFd::borrow_raw(fd),
                        rustix::io::FdFlags::empty(),
                    )
                    .map_err(io::Error::from)
                });
            }
        }
        // Windows children are contained by the supervising launcher's job instead.
        #[cfg(not(unix))]
        let _ = (&self.file, command);
    }
}

/// Exclusive proof that no installed process runs; dropping it lets games start again.
#[derive(Debug)]
pub struct Quiescent {
    _file: File,
}

impl Quiescent {
    /// Takes the gate without waiting; `WouldBlock` means a game process is still running.
    pub fn try_acquire(update_dir: &Path) -> io::Result<Self> {
        let file = open(update_dir)?;
        file.try_lock()?;
        Ok(Self { _file: file })
    }

    /// Waits for a closing game tree, bounded by `timeout` on the caller's clock.
    pub fn wait(
        update_dir: &Path,
        timeout: Duration,
        elapsed: impl FnMut() -> Duration,
        pause: impl FnMut(),
    ) -> io::Result<Self> {
        wait(timeout, elapsed, pause, || Self::try_acquire(update_dir))
    }
}

/// True while any installed process holds the gate; unknown status counts as live.
pub fn is_live(update_dir: &Path) -> bool {
    Quiescent::try_acquire(update_dir).is_err()
}

fn wait<T>(
    timeout: Duration,
    mut elapsed: impl FnMut() -> Duration,
    mut pause: impl FnMut(),
    mut attempt: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    loop {
        match attempt() {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            result => return result,
        }
        if elapsed() >= timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Cinnabar is still running or updating",
            ));
        }
        pause();
    }
}

fn open(update_dir: &Path) -> io::Result<File> {
    std::fs::create_dir_all(update_dir)?;
    File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(update_dir.join(RUNTIME_LOCK))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_running_session_blocks_installation_until_it_exits() {
        let root = tempfile::tempdir().unwrap();
        let game = Session::try_join(root.path()).unwrap();
        let monitor = Session::try_join(root.path()).unwrap();
        assert!(is_live(root.path()));
        drop(game);
        // Any remaining member of the tree still holds the gate.
        assert_eq!(
            Quiescent::try_acquire(root.path()).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(monitor);
        assert!(!is_live(root.path()));
        let install = Quiescent::try_acquire(root.path()).unwrap();
        assert_eq!(
            Session::try_join(root.path()).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(install);
        assert!(Session::try_join(root.path()).is_ok());
    }

    #[test]
    fn installers_give_up_on_a_live_game_after_the_callers_timeout() {
        let root = tempfile::tempdir().unwrap();
        let _game = Session::try_join(root.path()).unwrap();
        let elapsed = Cell::new(Duration::ZERO);
        let error = Quiescent::wait(
            root.path(),
            QUIESCE_TIMEOUT,
            || elapsed.get(),
            || elapsed.set(elapsed.get() + Duration::from_secs(1)),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(elapsed.get(), QUIESCE_TIMEOUT);
    }

    #[test]
    fn an_installer_proceeds_once_the_closing_tree_exits() {
        let root = tempfile::tempdir().unwrap();
        let game = std::cell::RefCell::new(Some(Session::try_join(root.path()).unwrap()));
        let gate = Quiescent::wait(
            root.path(),
            QUIESCE_TIMEOUT,
            || Duration::ZERO,
            || drop(game.borrow_mut().take()),
        )
        .unwrap();
        assert!(Session::try_join(root.path()).is_err());
        drop(gate);
    }

    #[cfg(unix)]
    #[test]
    fn a_shared_child_keeps_the_gate_after_its_parent_share_closes() {
        let root = tempfile::tempdir().unwrap();
        let session = Session::try_join(root.path()).unwrap();
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "read line"])
            .stdin(std::process::Stdio::piped());
        session.share_with(&mut command);
        let mut child = command.spawn().unwrap();
        drop(session);
        assert!(
            is_live(root.path()),
            "the child still runs from the install"
        );
        drop(child.stdin.take());
        child.wait().unwrap();
        assert!(!is_live(root.path()));
    }

    #[cfg(unix)]
    #[test]
    fn unshared_children_never_hold_the_gate() {
        let root = tempfile::tempdir().unwrap();
        let session = Session::try_join(root.path()).unwrap();
        let mut child = Command::new("/bin/sh")
            .args(["-c", "read line"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        drop(session);
        assert!(!is_live(root.path()));
        drop(child.stdin.take());
        child.wait().unwrap();
    }
}
