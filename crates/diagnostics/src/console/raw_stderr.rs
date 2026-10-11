//! Drains the process stderr pipe into the same ordered, rotating sink as queued output.
use super::{FULL_QUEUE_RETRY, Message, Stream};
use std::{
    fs::File,
    io::{self, Read},
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, TrySendError},
    },
    time::{Duration, Instant},
};

const READ_SIZE: usize = 4096;
const POLL_INTERVAL: Duration = Duration::from_millis(5);

pub(super) struct RawStderr {
    reader: Mutex<File>,
    cancelled: AtomicBool,
}

impl RawStderr {
    /// Installs a process-lifetime pipe only after its drain thread has started successfully.
    pub(super) fn start(queue: SyncSender<Message>) -> io::Result<Arc<Self>> {
        let (reader, writer) = pipe()?;
        let capture = Arc::new(Self {
            reader: Mutex::new(reader),
            cancelled: AtomicBool::new(false),
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
        Ok(capture)
    }

    /// Queues all bytes currently in the pipe before a console flush, respecting exit deadlines.
    pub(super) fn drain(&self, queue: &SyncSender<Message>, deadline: Option<Instant>) -> bool {
        let mut reader = loop {
            match self.reader.try_lock() {
                Ok(reader) => break reader,
                Err(TryLockError::Poisoned(_)) => return false,
                Err(TryLockError::WouldBlock) => {
                    if deadline.is_some_and(|end| Instant::now() >= end) {
                        return false;
                    }
                    std::thread::sleep(FULL_QUEUE_RETRY);
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
                Ok(0) | Err(_) => return false,
                Ok(count) => count,
            };
            let mut message = Message::Write(Stream::Stderr, bytes[..count].to_vec());
            loop {
                match queue.try_send(message) {
                    Ok(()) => break,
                    Err(TrySendError::Disconnected(_)) => return false,
                    Err(TrySendError::Full(returned)) => {
                        message = returned;
                        if deadline.is_some_and(|end| Instant::now() >= end) {
                            return false;
                        }
                        std::thread::sleep(FULL_QUEUE_RETRY);
                    }
                }
            }
        }
    }
}

/// Creates owned pipe ends; the read side never blocks when queried under the reader lock.
#[cfg(unix)]
fn pipe() -> io::Result<(File, File)> {
    let (read, write) = rustix::pipe::pipe()?;
    for fd in [&read, &write] {
        rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::CLOEXEC)?;
    }
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
