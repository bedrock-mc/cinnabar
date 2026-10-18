//! Drains the process stderr pipe into the same ordered, rotating sink as queued output.
use super::{
    Message, Priority, Stream,
    queue::{Queue, Rejected},
};
use std::{
    fs::File,
    io::{self, Read},
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Pause between attempts to take the pipe reader from the drain thread.
const READER_RETRY: Duration = Duration::from_millis(1);

const READ_SIZE: usize = 4096;
const POLL_INTERVAL: Duration = Duration::from_millis(5);

pub(super) struct RawStderr {
    /// The capture pipe's read end; whoever holds it is the only reader besides a fatal-signal
    /// drain.
    pub(super) reader: Mutex<File>,
    cancelled: AtomicBool,
    dropped: Arc<AtomicU64>, // Shared with the console so overflow is reported once, in order.
}

impl RawStderr {
    /// Installs a process-lifetime pipe only after its drain thread has started successfully.
    pub(super) fn start(queue: Arc<Queue>, dropped: Arc<AtomicU64>) -> io::Result<Arc<Self>> {
        let (reader, writer) = pipe()?;
        #[cfg(unix)]
        let reader_fd = std::os::fd::AsRawFd::as_raw_fd(&reader);
        let capture = Arc::new(Self {
            reader: Mutex::new(reader),
            cancelled: AtomicBool::new(false),
            dropped,
        });
        let thread = Arc::clone(&capture);
        std::thread::Builder::new()
            .name("stderr-capture".into())
            .spawn(move || {
                while !thread.cancelled.load(Ordering::Relaxed) {
                    if !thread.drain(&queue, None) {
                        break;
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
            })?;
        if let Err(error) = redirect(&writer) {
            capture.cancelled.store(true, Ordering::Relaxed);
            return Err(error);
        }
        // The console keeps the capture, and so the pipe, for the rest of the process.
        #[cfg(unix)]
        super::signal_drain::publish_reader(reader_fd);
        Ok(capture)
    }

    /// Queues all bytes currently in the pipe. Raw stderr carries panics and fatal reports, so it
    /// may use the queue's reserve. Without a deadline a full queue drops and counts the chunk,
    /// so a stalled console never backs up the pipe into `eprintln!` callers.
    pub(super) fn drain(&self, queue: &Queue, deadline: Option<Instant>) -> bool {
        let mut reader = loop {
            match self.reader.try_lock() {
                Ok(reader) => break reader,
                Err(TryLockError::Poisoned(_)) => return false,
                Err(TryLockError::WouldBlock) => {
                    if deadline.is_some_and(|end| Instant::now() >= end) {
                        return false;
                    }
                    std::thread::sleep(READER_RETRY);
                }
            }
        };
        let mut bytes = [0; READ_SIZE];
        loop {
            if deadline.is_some_and(|end| Instant::now() >= end) {
                return false;
            }
            let count = match available(&reader) {
                Ok(count) => count.min(bytes.len()),
                Err(_) => return false,
            };
            if count == 0 {
                return true;
            }
            let count = match reader.read(&mut bytes[..count]) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return true,
                Ok(0) | Err(_) => return false,
                Ok(count) => count,
            };
            let message = Message::Write {
                stream: Stream::Stderr,
                priority: Priority::Warn,
                bytes: bytes[..count].to_vec(),
            };
            let queued = match deadline {
                Some(end) => queue.push_until(message, end),
                None => queue.push(message),
            };
            match queued {
                Ok(()) => {}
                Err(Rejected::Closed(_)) => return false,
                Err(Rejected::Full) if deadline.is_some() => return false,
                Err(Rejected::Full) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Keeps an owned copy of the launcher's stderr destination before redirecting fd 2.
#[cfg(unix)]
pub(super) fn original() -> io::Result<Option<File>> {
    let fd = rustix::io::dup(rustix::stdio::stderr())?;
    rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC)?;
    Ok(Some(fd.into()))
}

/// Copies the Windows stderr handle, or returns no destination for a launch without a console.
#[cfg(windows)]
pub(super) fn original() -> io::Result<Option<File>> {
    use std::os::windows::io::BorrowedHandle;
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE};
    // SAFETY: GetStdHandle returns the process-owned standard-error handle.
    let handle = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if handle.is_null() {
        return Ok(None);
    }
    if handle as isize == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: The validated process handle remains open while its owned copy is created.
    Ok(Some(
        unsafe { BorrowedHandle::borrow_raw(handle) }
            .try_clone_to_owned()?
            .into(),
    ))
}

/// Creates owned pipe ends with a nonblocking reader, including when a signal consumes bytes.
#[cfg(unix)]
fn pipe() -> io::Result<(File, File)> {
    let (read, write) = rustix::pipe::pipe()?;
    for fd in [&read, &write] {
        rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::CLOEXEC)?;
    }
    let flags = rustix::fs::fcntl_getfl(&read)?;
    rustix::fs::fcntl_setfl(&read, flags | rustix::fs::OFlags::NONBLOCK)?;
    Ok((read.into(), write.into()))
}

/// Replaces fd 2 without closing the descriptor Rust assumes remains valid.
#[cfg(unix)]
fn redirect(writer: &File) -> io::Result<()> {
    rustix::stdio::dup2_stderr(writer)?;
    Ok(())
}

/// Counts unread bytes while the sole reader is locked, so the subsequent read cannot block.
#[cfg(unix)]
fn available(reader: &File) -> io::Result<usize> {
    Ok(rustix::io::ioctl_fionread(reader)? as usize)
}

/// Creates an anonymous Windows pipe whose handles are owned by the returned files.
#[cfg(windows)]
fn pipe() -> io::Result<(File, File)> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::System::Pipes::CreatePipe;
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    // SAFETY: CreatePipe receives valid output pointers and no inherited security attributes.
    if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: These are newly created handles, transferred exactly once into owned files.
    Ok(unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) })
}

/// Redirects both the C runtime descriptor and the Windows handle used by Rust's stderr.
#[cfg(windows)]
fn redirect(writer: &File) -> io::Result<()> {
    use std::os::windows::io::{FromRawHandle, IntoRawHandle};
    use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, SetStdHandle};
    let handle = writer.try_clone()?.into_raw_handle();
    // SAFETY: The duplicated owned handle is transferred to the CRT on success.
    let fd = unsafe { libc::open_osfhandle(handle as isize, libc::O_BINARY) };
    if fd < 0 {
        // SAFETY: A failed ownership transfer leaves the duplicated handle ours to close.
        drop(unsafe { File::from_raw_handle(handle) });
        return Err(io::Error::last_os_error());
    }
    if fd != 2 {
        // SAFETY: fd is valid; descriptor 2 owns its duplicate after a successful call.
        let result = unsafe { libc::dup2(fd, 2) };
        let error = io::Error::last_os_error();
        // SAFETY: Only the temporary descriptor is closed, never the destination fd 2.
        unsafe {
            libc::close(fd);
        }
        if result < 0 {
            return Err(error);
        }
    }
    // SAFETY: Descriptor 2 supplies a process-lifetime handle for the standard-error slot.
    if unsafe { SetStdHandle(STD_ERROR_HANDLE, libc::get_osfhandle(2) as _) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Counts unread bytes on the pipe without waiting for another stderr write.
#[cfg(windows)]
fn available(reader: &File) -> io::Result<usize> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;
    let mut count = 0;
    // SAFETY: The file owns a valid pipe handle, and count is a valid output pointer.
    if unsafe {
        PeekNamedPipe(
            reader.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut count,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(count as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::native::queue::{RESERVED_CAPACITY, SHARED_CAPACITY, SystemClock};
    use std::io::Write;

    /// A stalled console must not stop the pipe draining, or raw stderr writers block.
    #[test]
    fn a_full_queue_drops_pipe_bytes_instead_of_waiting() {
        let (reader, mut writer) = pipe().unwrap();
        let capture = RawStderr {
            reader: Mutex::new(reader),
            cancelled: AtomicBool::new(false),
            dropped: Arc::new(AtomicU64::new(0)),
        };
        let queue = Queue::new(Box::new(SystemClock));
        for _ in 0..SHARED_CAPACITY + RESERVED_CAPACITY {
            let queued = queue.push(Message::Write {
                stream: Stream::Stderr,
                priority: Priority::Warn,
                bytes: b"queued".to_vec(),
            });
            assert!(queued.is_ok());
        }
        writer.write_all(b"stalled console output").unwrap();
        assert!(capture.drain(&queue, None));
        assert_eq!(capture.dropped.load(Ordering::Relaxed), 1);
        assert_eq!(available(&capture.reader.lock().unwrap()).unwrap(), 0);
    }
}

#[cfg(all(test, unix))]
mod nonblocking_tests {
    use super::*;

    /// Models a competing reader consuming all bytes before the fatal handler's next read.
    #[test]
    fn exhausted_reader_fixture() {
        if std::env::var_os("CINNABAR_EMPTY_CAPTURE_FIXTURE").is_none() {
            return;
        }
        use std::io::Write;
        let (mut reader, mut writer) = pipe().unwrap();
        writer.write_all(b"x").unwrap();
        let mut byte = [0];
        reader.read_exact(&mut byte).unwrap();
        assert_eq!(
            reader.read(&mut byte).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    /// An exhausted capture reader returns immediately while its write end remains open.
    #[test]
    fn exhausted_capture_pipe_never_waits_for_another_stderr_write() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "console::native::raw_stderr::nonblocking_tests::exhausted_reader_fixture",
            ])
            .env("CINNABAR_EMPTY_CAPTURE_FIXTURE", "1")
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
            "capture reader blocked after its last byte was consumed"
        );
        assert!(status.unwrap().success());
    }
}
