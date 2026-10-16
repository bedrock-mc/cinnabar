//! Conservative process visibility for session reclamation.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OwnerState {
    Alive,
    Dead,
    Unknown,
}

/// Proves absence only through a successful OS query; permission errors preserve data.
#[cfg(unix)]
pub(super) fn owner_state(pid: u32) -> OwnerState {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return OwnerState::Unknown;
    };
    if pid <= 0 {
        return OwnerState::Unknown;
    }
    // SAFETY: the positive PID selects one process, and signal zero probes without sending a signal.
    if unsafe { libc::kill(pid, 0) } == 0 {
        OwnerState::Alive
    } else if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        OwnerState::Dead
    } else {
        OwnerState::Unknown
    }
}

/// Uses a complete process listing; an error or a full bounded buffer is indeterminate.
#[cfg(windows)]
pub(super) fn owner_state(pid: u32) -> OwnerState {
    use windows_sys::Win32::System::ProcessStatus::K32EnumProcesses;
    const INITIAL_PROCESS_CAPACITY: usize = 256;
    const MAX_PROCESS_CAPACITY: usize = 65_536;
    if pid == 0 {
        return OwnerState::Unknown;
    }
    let mut pids = vec![0_u32; INITIAL_PROCESS_CAPACITY];
    loop {
        let bytes =
            u32::try_from(std::mem::size_of_val(pids.as_slice())).expect("bounded PID buffer");
        let mut written = 0;
        // SAFETY: pids is writable for bytes and written is a live DWORD output.
        if unsafe { K32EnumProcesses(pids.as_mut_ptr(), bytes, &mut written) } == 0
            || written > bytes
            || !(written as usize).is_multiple_of(size_of::<u32>())
        {
            return OwnerState::Unknown;
        }
        let count = written as usize / size_of::<u32>();
        if pids[..count].contains(&pid) {
            return OwnerState::Alive;
        }
        if written < bytes {
            return OwnerState::Dead;
        }
        if pids.len() == MAX_PROCESS_CAPACITY {
            return OwnerState::Unknown;
        }
        pids.resize((pids.len() * 2).min(MAX_PROCESS_CAPACITY), 0);
    }
}

/// Unsupported process visibility never grants deletion authority.
#[cfg(not(any(unix, windows)))]
pub(super) fn owner_state(_: u32) -> OwnerState {
    OwnerState::Unknown
}
