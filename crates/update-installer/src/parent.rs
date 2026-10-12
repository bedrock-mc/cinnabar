//! Parent-exit barriers used only by the offline helper process.
use anyhow::{Result, ensure};

/// Waits for the pipe held by the client and for its actual process exit before installing.
pub fn wait(pid: u32) -> Result<()> {
    ensure!(
        pid != 0 && pid != std::process::id(),
        "invalid updater parent"
    );
    #[cfg(windows)]
    return windows_wait(pid);
    #[cfg(unix)]
    {
        use std::io::Read;
        std::io::stdin().take(1).read_to_end(&mut Vec::new())?;
        while rustix::process::getppid()
            .is_some_and(|parent| parent.as_raw_nonzero().get() as u32 == pid)
        {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    anyhow::bail!("unsupported parent wait")
}

#[cfg(windows)]
/// Holds a process handle, so PID reuse cannot let the helper replace a live parent.
fn windows_wait(pid: u32) -> Result<()> {
    use std::io::Read;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, WAIT_OBJECT_0},
        System::Threading::{INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    // SAFETY: the returned handle is checked, used only for waiting, and closed once.
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            ensure!(
                GetLastError() == ERROR_INVALID_PARAMETER,
                "cannot open updater parent"
            );
            // The parent may have finished before Windows scheduled this helper.
            std::io::stdin().take(1).read_to_end(&mut Vec::new())?;
            return Ok(());
        }
        let pipe = std::io::stdin().take(1).read_to_end(&mut Vec::new());
        let result = if pipe.is_ok() {
            WaitForSingleObject(process, INFINITE)
        } else {
            u32::MAX
        };
        CloseHandle(process);
        pipe?;
        ensure!(result == WAIT_OBJECT_0, "waiting for updater parent failed");
    }
    Ok(())
}
