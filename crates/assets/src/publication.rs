//! Atomic file and directory publication shared by asset and pack storage.

use std::{io, path::Path, time::Duration};

/// Longest total wait for a rename that another process briefly blocks.
const TRANSIENT_RETRY_BUDGET: Duration = Duration::from_secs(20);
/// Wait before the first retry; each later wait doubles up to [`MAX_RETRY_DELAY`].
const FIRST_RETRY_DELAY: Duration = Duration::from_millis(10);
/// Longest single wait between retries.
const MAX_RETRY_DELAY: Duration = Duration::from_millis(500);

/// Atomically moves a file or directory without replacing an existing destination.
///
/// Windows refuses to rename a directory while any file inside it is open, and antivirus
/// scanners and the search indexer open freshly written files for a moment. Those refusals are
/// retried with backoff for up to [`TRANSIENT_RETRY_BUDGET`]; every other error returns at once.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    retry_transient(
        || rename_once(from, to),
        is_transient_lock,
        std::thread::sleep,
    )
}

/// Runs `attempt` until it succeeds, fails with an error `transient` rejects, or the next wait
/// would take the total past [`TRANSIENT_RETRY_BUDGET`], then returns that last result. Waits
/// start at [`FIRST_RETRY_DELAY`], double up to [`MAX_RETRY_DELAY`] and go through `sleep`.
fn retry_transient<T>(
    mut attempt: impl FnMut() -> io::Result<T>,
    transient: impl Fn(&io::Error) -> bool,
    mut sleep: impl FnMut(Duration),
) -> io::Result<T> {
    let mut delay = FIRST_RETRY_DELAY;
    let mut waited = Duration::ZERO;
    loop {
        match attempt() {
            Err(error) if transient(&error) && waited + delay <= TRANSIENT_RETRY_BUDGET => {
                sleep(delay);
                waited += delay;
                delay = (delay * 2).min(MAX_RETRY_DELAY);
            }
            result => return result,
        }
    }
}

/// Whether Windows refused the rename because another process holds a handle in the tree.
#[cfg(windows)]
fn is_transient_lock(error: &io::Error) -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION};
    error.raw_os_error().is_some_and(|code| {
        code == ERROR_ACCESS_DENIED as i32 || code == ERROR_SHARING_VIOLATION as i32
    })
}

/// Other platforms rename regardless of open handles, so no error is transient.
#[cfg(not(windows))]
fn is_transient_lock(_: &io::Error) -> bool {
    false
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
/// One atomic no-replace move.
fn rename_once(from: &Path, to: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

#[cfg(windows)]
/// One atomic no-replace move.
fn rename_once(from: &Path, to: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    // No MOVEFILE_REPLACE_EXISTING: an existing destination fails the move.
    let (from, to) = (extended_wide(from)?, extended_wide(to)?);
    // SAFETY: both are NUL-terminated UTF-16 buffers that outlive the call.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// NUL-terminated `\\?\` form, so deep install paths are not cut at `MAX_PATH`.
#[cfg(windows)]
fn extended_wide(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let absolute = std::path::absolute(path)?;
    let text = absolute.as_os_str();
    let prefix: &str = match text.to_str() {
        Some(text) if text.starts_with(r"\\?\") => "",
        Some(text) if text.starts_with(r"\\") => r"\\?\UNC",
        _ => r"\\?\",
    };
    let body = if prefix == r"\\?\UNC" {
        absolute
            .to_str()
            .map_or_else(|| text.to_os_string(), |text| text[1..].into())
    } else {
        text.to_os_string()
    };
    Ok(std::ffi::OsStr::new(prefix)
        .encode_wide()
        .chain(body.encode_wide())
        .chain([0])
        .collect())
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    windows
)))]
/// One atomic no-replace move.
fn rename_once(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace directory publication is unsupported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locked(error: &io::Error) -> bool {
        error.kind() == io::ErrorKind::PermissionDenied
    }

    fn denied() -> io::Error {
        io::Error::from(io::ErrorKind::PermissionDenied)
    }

    #[test]
    fn a_briefly_locked_rename_succeeds_after_backing_off() {
        let (mut attempts, mut waits) = (0, Vec::new());
        let result = retry_transient(
            || {
                attempts += 1;
                if attempts <= 3 { Err(denied()) } else { Ok(()) }
            },
            locked,
            |delay| waits.push(delay),
        );
        result.unwrap();
        assert_eq!(attempts, 4);
        assert_eq!(
            waits,
            [10, 20, 40].map(Duration::from_millis),
            "waits double from the first delay"
        );
    }

    #[test]
    fn other_errors_return_without_waiting() {
        let (mut attempts, mut waits) = (0, Vec::new());
        let error = retry_transient::<()>(
            || {
                attempts += 1;
                Err(io::Error::from(io::ErrorKind::AlreadyExists))
            },
            locked,
            |delay| waits.push(delay),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(attempts, 1);
        assert!(waits.is_empty());
    }

    #[test]
    fn a_lasting_lock_fails_within_the_budget() {
        let (mut attempts, mut waits) = (0, Vec::<Duration>::new());
        let error = retry_transient::<()>(
            || {
                attempts += 1;
                Err(denied())
            },
            locked,
            |delay| waits.push(delay),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(attempts, waits.len() + 1, "one attempt after every wait");
        let waited: Duration = waits.iter().sum();
        assert!(waited <= TRANSIENT_RETRY_BUDGET, "{waited:?}");
        assert!(
            waited + MAX_RETRY_DELAY > TRANSIENT_RETRY_BUDGET,
            "keeps retrying until the budget is spent: {waited:?}"
        );
        assert!(waits.iter().all(|delay| *delay <= MAX_RETRY_DELAY));
    }
}
