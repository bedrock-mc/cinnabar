//! Copies raw stderr still waiting in the capture pipe when a fatal signal arrives, where the
//! drain thread will never run again: to the session log and an already nonblocking terminal.
//! Everything here is published before any signal and used without allocation or locks.
use std::{
    cell::UnsafeCell,
    fs::File,
    os::fd::{BorrowedFd, RawFd},
    sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering},
};

const CHUNK: usize = 4096;
/// Most bytes copied, a full default pipe: other threads may still be writing.
const LIMIT: usize = 64 << 10;

/// The capture pipe's read end, or -1 before raw stderr is captured.
static READER: AtomicI32 = AtomicI32::new(-1);
/// The launcher's stderr destination, kept open by the console writer, or -1.
static TERMINAL: AtomicI32 = AtomicI32::new(-1);
/// An owned append handle to this session's active segment, replaced outside signal handlers.
static LOG: AtomicPtr<File> = AtomicPtr::new(std::ptr::null_mut());
/// Admits one drain, the only user of [`BUFFER`].
static DRAINING: AtomicBool = AtomicBool::new(false);
static BUFFER: Buffer = Buffer(UnsafeCell::new([0; CHUNK]));

struct Buffer(UnsafeCell<[u8; CHUNK]>);

// SAFETY: Only the drain admitted by `DRAINING` touches the buffer.
unsafe impl Sync for Buffer {}

/// Records an already nonblocking terminal without changing launcher flags.
pub(super) fn publish_terminal(terminal: Option<RawFd>) {
    if let Some(terminal) = terminal {
        // Changing a duplicate's file-status flags would also change the launcher's original.
        // Only destinations already configured for nonblocking writes are safe in a handler.
        // SAFETY: The console owns this live destination while attaching its session files.
        let fd = unsafe { BorrowedFd::borrow_raw(terminal) };
        if rustix::fs::fcntl_getfl(fd)
            .is_ok_and(|flags| flags.contains(rustix::fs::OFlags::NONBLOCK))
        {
            TERMINAL.store(terminal, Ordering::Release);
        }
    }
}

/// Keeps an owned handle to the active session segment, even if another client renames it.
pub(crate) fn publish_log(file: &File) -> std::io::Result<()> {
    let next = Box::into_raw(Box::new(file.try_clone()?));
    let old = LOG.swap(next, Ordering::SeqCst);
    if !old.is_null() && !DRAINING.load(Ordering::SeqCst) {
        // SAFETY: A drain announces itself before loading LOG. Any later drain sees next.
        drop(unsafe { Box::from_raw(old) });
    }
    Ok(())
}

/// Records the nonblocking capture reader; its pipe lives for the rest of the process.
pub(super) fn publish_reader(reader: RawFd) {
    READER.store(reader, Ordering::Release);
}

/// Copies what the capture pipe holds now; async-signal-safe and runs at most once.
pub(super) fn drain() {
    let reader = READER.load(Ordering::Acquire);
    if reader < 0 || DRAINING.swap(true, Ordering::SeqCst) {
        return;
    }
    // SAFETY: The pipe and the launcher's stderr stay open for the process lifetime.
    let reader = unsafe { BorrowedFd::borrow_raw(reader) };
    let terminal = TERMINAL.load(Ordering::Acquire);
    // SAFETY: As above; -1 means no destination was published.
    let terminal = (terminal >= 0).then(|| unsafe { BorrowedFd::borrow_raw(terminal) });
    let log = LOG.load(Ordering::SeqCst);
    // SAFETY: Once DRAINING is set, publishers keep every possibly observed handle alive.
    let log = unsafe { log.as_ref() };
    // SAFETY: `DRAINING` admitted only this call.
    let buffer = unsafe { &mut *BUFFER.0.get() };
    let mut copied = 0;
    while copied < LIMIT {
        let count = match rustix::io::read(reader, &mut buffer[..CHUNK.min(LIMIT - copied)]) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        if let Some(terminal) = terminal {
            write_all(terminal, &buffer[..count]);
        }
        if let Some(log) = log {
            write_all(log, &buffer[..count]);
        }
        copied += count;
    }
}

/// Attempts nonblocking writes until complete, out of room, or an OS error.
fn write_all(fd: impl rustix::fd::AsFd, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match rustix::io::write(&fd, bytes) {
            Ok(0) | Err(_) => return,
            Ok(count) => bytes = &bytes[count..],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::{
        fd::OwnedFd,
        fs::{OFlags, fcntl_getfl, fcntl_setfl},
        io::Errno,
    };
    use std::os::fd::AsRawFd;

    /// Creates fixture pipes with nonblocking ends on every supported Unix platform.
    fn nonblocking_pipe() -> (OwnedFd, OwnedFd) {
        let (reader, writer) = rustix::pipe::pipe().unwrap();
        for fd in [&reader, &writer] {
            let flags = fcntl_getfl(fd).unwrap();
            fcntl_setfl(fd, flags | OFlags::NONBLOCK).unwrap();
        }
        (reader, writer)
    }

    /// Leaves a blocking launcher pipe full before the fatal path attempts to forward bytes.
    #[test]
    fn full_terminal_fixture() {
        let Some(root) = std::env::var_os("CINNABAR_FULL_TERMINAL_FIXTURE") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let (_terminal_reader, terminal) = nonblocking_pipe();
        let chunk = [b'x'; 4096];
        loop {
            match rustix::io::write(&terminal, &chunk) {
                Ok(_) => {}
                Err(Errno::AGAIN) => break,
                result => panic!("cannot fill fixture pipe: {result:?}"),
            }
        }
        let flags = fcntl_getfl(&terminal).unwrap();
        fcntl_setfl(&terminal, flags & !OFlags::NONBLOCK).unwrap();
        let (reader, writer) = nonblocking_pipe();
        rustix::io::write(&writer, b"fatal marker\n").unwrap();
        let log = root.join("client.log");
        std::fs::write(&log, b"header\n").unwrap();
        publish_terminal(Some(terminal.as_raw_fd()));
        publish_log(&std::fs::OpenOptions::new().append(true).open(&log).unwrap()).unwrap();
        publish_reader(reader.as_raw_fd());
        drain();
    }

    /// A stalled launcher's terminal cannot stop the fatal path from preserving its log bytes.
    #[test]
    fn a_full_blocking_terminal_cannot_hold_the_fatal_drain() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "console::native::signal_drain::tests::full_terminal_fixture",
            ])
            .env("CINNABAR_FULL_TERMINAL_FIXTURE", dir.path())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut status = None;
        let completed = test_time::wait_until(test_time::DEFAULT_TIMEOUT, || {
            status = child.try_wait().unwrap();
            status.is_some()
        });
        if !completed {
            child.kill().unwrap();
            child.wait().unwrap();
        }
        assert!(
            completed,
            "fatal drain blocked on the launcher's full terminal"
        );
        assert!(status.unwrap().success());
        assert_eq!(
            std::fs::read(dir.path().join("client.log")).unwrap(),
            b"header\nfatal marker\n"
        );
    }
}
