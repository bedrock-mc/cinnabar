//! Bounded retries for Windows handles that briefly prevent asset directory swaps.

use std::{fs, io, path::Path};

#[cfg(any(windows, test))]
use std::time::Duration;

#[cfg(any(windows, test))]
const RETRIES: usize = 50;
#[cfg(any(windows, test))]
const RETRY_DELAY: Duration = Duration::from_millis(100);

/// Renames a directory after transient Windows locks have had time to clear.
pub(super) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    run(|| fs::rename(from, to))
}

/// Removes an old carrier tree after transient Windows locks have had time to clear.
pub(super) fn remove_dir_all(path: &Path) -> io::Result<()> {
    run(|| fs::remove_dir_all(path))
}

/// Leaves non-Windows filesystem errors unchanged and unretried.
fn run(operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    #[cfg(windows)]
    {
        retry_with(operation, std::thread::sleep)
    }
    #[cfg(not(windows))]
    {
        let mut operation = operation;
        operation()
    }
}

/// Retries only Windows access-denied, sharing-violation, and lock-violation errors.
#[cfg(any(windows, test))]
fn retry_with(
    mut operation: impl FnMut() -> io::Result<()>,
    mut wait: impl FnMut(Duration),
) -> io::Result<()> {
    for attempt in 0..=RETRIES {
        match operation() {
            Err(error)
                if attempt < RETRIES && matches!(error.raw_os_error(), Some(5 | 32 | 33)) =>
            {
                wait(RETRY_DELAY);
            }
            result => return result,
        }
    }
    unreachable!("the final attempt always returns")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_swap_survives_transient_windows_locks() {
        for code in [5, 32, 33] {
            let mut attempts = 0;
            let mut waits = 0;
            let result = retry_with(
                || {
                    attempts += 1;
                    if attempts < 3 {
                        Err(io::Error::from_raw_os_error(code))
                    } else {
                        Ok(())
                    }
                },
                |_| waits += 1,
            );
            result.unwrap();
            assert_eq!(attempts, 3);
            assert_eq!(waits, 2);
        }
    }

    #[test]
    fn persistent_locks_return_the_last_error_after_a_bounded_retry() {
        let mut attempts = 0;
        let mut waits = 0;
        let error = retry_with(
            || {
                attempts += 1;
                Err(io::Error::from_raw_os_error(if attempts == 1 {
                    5
                } else {
                    32
                }))
            },
            |_| waits += 1,
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(32));
        assert_eq!(attempts, RETRIES + 1);
        assert_eq!(waits, RETRIES);
    }

    #[test]
    fn unrelated_errors_fail_without_waiting() {
        for error in [
            io::Error::from_raw_os_error(2),
            io::Error::from_raw_os_error(17),
            io::Error::new(io::ErrorKind::PermissionDenied, "not a Windows lock"),
        ] {
            let raw = error.raw_os_error();
            let mut error = Some(error);
            let result = retry_with(
                || Err(error.take().expect("an unrelated failure must not retry")),
                |_| panic!("an unrelated failure must not wait"),
            );
            assert_eq!(result.unwrap_err().raw_os_error(), raw);
        }
    }

    #[test]
    fn a_successful_operation_does_not_wait() {
        retry_with(|| Ok(()), |_| panic!("success must not wait")).unwrap();
    }

    #[test]
    fn an_unrelated_error_after_a_lock_stops_the_retry() {
        let mut attempts = 0;
        let result = retry_with(
            || {
                attempts += 1;
                Err(io::Error::from_raw_os_error(if attempts == 1 {
                    5
                } else {
                    2
                }))
            },
            |_| {},
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(2));
        assert_eq!(attempts, 2);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_errors_are_not_interpreted_as_windows_locks() {
        let mut attempts = 0;
        let error = run(|| {
            attempts += 1;
            Err(io::Error::from_raw_os_error(5))
        })
        .unwrap_err();
        assert_eq!(attempts, 1);
        assert_eq!(error.raw_os_error(), Some(5));
    }

    #[cfg(windows)]
    #[test]
    fn a_real_windows_directory_lock_can_clear_during_retry() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("compiled");
        let to = root.path().join("compiled.previous");
        fs::create_dir(&from).unwrap();
        let carrier = from.join("carrier");
        fs::write(&carrier, b"old assets").unwrap();
        let mut handle = Some(
            fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(carrier)
                .unwrap(),
        );
        let mut waits = 0;
        retry_with(
            || fs::rename(&from, &to),
            |_| {
                waits += 1;
                drop(handle.take());
            },
        )
        .unwrap();
        assert!(waits > 0, "the open handle must prevent the first rename");
        assert_eq!(fs::read(to.join("carrier")).unwrap(), b"old assets");
    }
}
