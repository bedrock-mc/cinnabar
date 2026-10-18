//! Parent-exit barriers used only by the offline helper process.
use anyhow::{Result, ensure};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::transfer_monitor;

/// Waits for the pipe held by the client and for its actual process exit before installing.
pub fn wait(pid: u32) -> Result<()> {
    ensure!(
        pid != 0 && pid != std::process::id(),
        "invalid updater parent"
    );
    #[cfg(windows)]
    return windows::wait(pid);
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

/// Keeps transferred process identities through pipe closure and waits for every image to unload.
#[cfg(any(windows, test))]
fn exit_barrier<T>(
    processes: impl IntoIterator<Item = Option<T>>,
    pipe_closed: impl FnOnce() -> Result<()>,
    mut wait: impl FnMut(T) -> Result<()>,
) -> Result<()> {
    pipe_closed()?;
    for process in processes.into_iter().flatten() {
        wait(process)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn a_reaped_monitor_is_not_reopened_by_pid_during_handoff() {
        let mut waited = Vec::new();
        let open_parent = |pid| {
            ensure!(pid == 10, "the monitor PID now belongs to another process");
            Ok::<_, anyhow::Error>(Some(pid))
        };
        let transferred_monitor = Some(20);
        exit_barrier(
            [open_parent(10).unwrap(), transferred_monitor],
            || Ok(()),
            |process| {
                waited.push(process);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(waited, [10, 20]);
    }

    #[test]
    fn client_and_monitor_exits_precede_installation() {
        let events = RefCell::new(Vec::new());
        exit_barrier(
            [Some(10), Some(20)],
            || {
                events.borrow_mut().push(("pipe closed", 0));
                Ok(())
            },
            |pid| {
                events.borrow_mut().push(("exited", pid));
                Ok(())
            },
        )
        .unwrap();
        events.borrow_mut().push(("install", 0));
        assert_eq!(
            *events.borrow(),
            [
                ("pipe closed", 0),
                ("exited", 10),
                ("exited", 20),
                ("install", 0),
            ]
        );
    }

    #[test]
    fn a_missing_process_is_already_gone_but_a_failed_wait_blocks_installation() {
        let mut waited = Vec::new();
        assert!(
            exit_barrier(
                [None, Some(20), Some(30)],
                || Ok(()),
                |pid| {
                    waited.push(pid);
                    anyhow::bail!("cannot observe process exit")
                }
            )
            .is_err()
        );
        assert_eq!(waited, [20]);
    }
}
