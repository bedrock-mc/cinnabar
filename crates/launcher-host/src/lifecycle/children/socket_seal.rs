//! Speculative descriptor inspection uses raw syscalls, without manufacturing Rust borrows.

use std::{io, mem::MaybeUninit, os::fd::RawFd};

/// Marks a socket close-on-exec; invalid descriptors return the OS error.
/// Other threads may still close or reuse the descriptor between syscalls.
pub(super) fn seal(raw: RawFd) -> io::Result<()> {
    // SAFETY: F_GETFD accepts arbitrary descriptor integers and reports EBADF for closed slots.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if flags & libc::FD_CLOEXEC != 0 {
        return Ok(());
    }
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstat accepts arbitrary descriptor integers and a writable stat buffer.
    if unsafe { libc::fstat(raw, stat.as_mut_ptr()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful fstat initialized the buffer.
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT == libc::S_IFSOCK {
        // SAFETY: F_SETFD accepts arbitrary descriptor integers and integer descriptor flags.
        if unsafe { libc::fcntl(raw, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::io::{FdFlags, fcntl_getfd, fcntl_setfd};
    use std::os::fd::AsRawFd;

    #[test]
    fn invalid_descriptors_report_ebadf() {
        assert_eq!(seal(-1).unwrap_err().raw_os_error(), Some(libc::EBADF));
    }

    #[test]
    fn only_socket_descriptors_gain_close_on_exec() {
        let (socket, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let file = tempfile::tempfile().unwrap();
        for fd in [&socket as &dyn AsRawFd, &file as &dyn AsRawFd] {
            let raw = fd.as_raw_fd();
            // SAFETY: both resources remain owned throughout the calls.
            assert_eq!(unsafe { libc::fcntl(raw, libc::F_SETFD, 0) }, 0);
            seal(raw).unwrap();
        }
        assert!(fcntl_getfd(&socket).unwrap().contains(FdFlags::CLOEXEC));
        assert!(fcntl_getfd(&file).unwrap().is_empty());
        fcntl_setfd(&file, FdFlags::CLOEXEC).unwrap();
    }
}
