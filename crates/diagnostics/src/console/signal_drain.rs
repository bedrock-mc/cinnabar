//! Copies raw stderr still waiting in the capture pipe when a fatal signal arrives, where the
//! drain thread will never run again: to the launcher's stderr and the end of the session log.
//! Everything here is published before any signal and used without allocation or locks.
use std::{
    cell::UnsafeCell,
    ffi::CString,
    os::fd::{BorrowedFd, RawFd},
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
};

const CHUNK: usize = 4096;
/// Most bytes copied, a full default pipe: other threads may still be writing.
const LIMIT: usize = 64 << 10;

/// The capture pipe's read end, or -1 before raw stderr is captured.
static READER: AtomicI32 = AtomicI32::new(-1);
/// The launcher's stderr destination, kept open by the console writer, or -1.
static TERMINAL: AtomicI32 = AtomicI32::new(-1);
/// The session log's current path.
static LOG_PATH: OnceLock<CString> = OnceLock::new();
/// Admits one drain, the only user of [`BUFFER`].
static DRAINING: AtomicBool = AtomicBool::new(false);
static BUFFER: Buffer = Buffer(UnsafeCell::new([0; CHUNK]));

struct Buffer(UnsafeCell<[u8; CHUNK]>);

// SAFETY: Only the drain admitted by `DRAINING` touches the buffer.
unsafe impl Sync for Buffer {}

/// Records where a signal-time drain writes; called once while attaching session files.
pub(super) fn publish_destinations(terminal: Option<RawFd>, log_path: &std::path::Path) {
    use std::os::unix::ffi::OsStrExt;
    if let Some(terminal) = terminal {
        TERMINAL.store(terminal, Ordering::Release);
    }
    if let Ok(path) = CString::new(log_path.as_os_str().as_bytes()) {
        let _ = LOG_PATH.set(path);
    }
}

/// Records the capture pipe's read end once raw stderr is captured; the pipe lives for the
/// rest of the process.
pub(super) fn publish_reader(reader: RawFd) {
    READER.store(reader, Ordering::Release);
}

/// Copies what the capture pipe holds now; async-signal-safe and runs at most once.
pub(super) fn drain() {
    let reader = READER.load(Ordering::Acquire);
    if reader < 0 || DRAINING.swap(true, Ordering::AcqRel) {
        return;
    }
    // SAFETY: The pipe and the launcher's stderr stay open for the process lifetime.
    let reader = unsafe { BorrowedFd::borrow_raw(reader) };
    let terminal = TERMINAL.load(Ordering::Acquire);
    // SAFETY: As above; -1 means no destination was published.
    let terminal = (terminal >= 0).then(|| unsafe { BorrowedFd::borrow_raw(terminal) });
    // `open` with a prebuilt C path neither allocates nor locks; O_APPEND keeps these bytes
    // after whatever the console writer has written.
    let log = LOG_PATH.get().and_then(|path| {
        rustix::fs::open(
            path.as_c_str(),
            rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::APPEND | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .ok()
    });
    // SAFETY: `DRAINING` admitted only this call.
    let buffer = unsafe { &mut *BUFFER.0.get() };
    let mut copied = 0;
    while copied < LIMIT {
        let available = match rustix::io::ioctl_fionread(reader) {
            Ok(0) | Err(_) => break,
            Ok(count) => usize::try_from(count).unwrap_or(CHUNK).min(CHUNK),
        };
        let count = match rustix::io::read(reader, &mut buffer[..available]) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        if let Some(terminal) = terminal {
            write_all(terminal, &buffer[..count]);
        }
        if let Some(log) = &log {
            write_all(log, &buffer[..count]);
        }
        copied += count;
    }
}

fn write_all(fd: impl rustix::fd::AsFd, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match rustix::io::write(&fd, bytes) {
            Ok(0) | Err(_) => return,
            Ok(count) => bytes = &bytes[count..],
        }
    }
}
