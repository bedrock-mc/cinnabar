//! Refuse inherited administrator rights before startup or a client relaunch.

use std::io;

/// Runs `action` only at the user's normal Windows permission level. A failed token query
/// also refuses the action; `asInvoker` alone does not lower an elevated parent's token.
pub fn normal_user<T>(action: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    with_elevation(elevated(), action)
}

/// Applies the same fail-closed permission decision to startup and every relaunch.
fn with_elevation<T>(
    elevated: io::Result<bool>,
    action: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    if elevated? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "This program cannot run as administrator. Close it and open it normally from the Start menu. Disable 'Run this program as an administrator' in its compatibility settings.",
        ));
    }
    action()
}

/// Reads this process's primary token, closing the query handle on every path.
#[cfg(windows)]
fn elevated() -> io::Result<bool> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    let mut elevation = TOKEN_ELEVATION::default();
    let mut length = 0;
    // SAFETY: the API writes into valid outputs. The successful token handle is closed once,
    // after preserving any query error, and is never inherited by a child.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let read = GetTokenInformation(
            token,
            TokenElevation,
            std::ptr::from_mut(&mut elevation).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        );
        let result = if read == 0 {
            Err(io::Error::last_os_error())
        } else if length != size_of::<TOKEN_ELEVATION>() as u32 {
            Err(io::Error::other(
                "Windows returned an invalid elevation token",
            ))
        } else {
            Ok(elevation.TokenIsElevated != 0)
        };
        CloseHandle(token);
        result
    }
}

/// Other platforms retain their existing startup permissions.
#[cfg(not(windows))]
fn elevated() -> io::Result<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevated_relaunch_never_starts_the_client() {
        let error = with_elevation(Ok(true), || -> io::Result<()> {
            panic!("an elevated parent must never start the client")
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn unknown_permissions_never_start_the_client() {
        let error = with_elevation(
            Err(io::Error::other("token query failed")),
            || -> io::Result<()> {
                panic!("unknown parent permissions must never start the client")
            },
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "token query failed");
    }

    #[test]
    fn normal_permissions_run_once_and_preserve_spawn_errors() {
        let mut starts = 0;
        assert_eq!(
            with_elevation(Ok(false), || {
                starts += 1;
                Ok(42)
            })
            .unwrap(),
            42
        );
        assert_eq!(starts, 1);
        assert_eq!(
            with_elevation(Ok(false), || Err::<(), _>(io::Error::other("spawn failed")))
                .unwrap_err()
                .to_string(),
            "spawn failed"
        );
    }
}
