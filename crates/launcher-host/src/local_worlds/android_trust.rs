//! Refreshes the Android server's platform trust before client startup can continue.

use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use super::{android_server_trust_path, create_private_dir};

/// Publishes a nonempty platform export or invalidates the previous bundle. Returns whether
/// current trust is available; an error must stop setup because old trust could not be removed.
pub fn refresh(user_data_root: &Path, pem: Option<&[u8]>) -> io::Result<bool> {
    let path = android_server_trust_path(user_data_root);
    let published = pem
        .filter(|bytes| !bytes.is_empty())
        .ok_or_else(|| io::Error::other("platform trust export unavailable"))
        .and_then(|bytes| replace(&path, bytes));
    finish_refresh(&path, published, |path| fs::remove_file(path))
}

/// Allows setup without exported trust only after the previous bundle has been invalidated.
fn finish_refresh(
    path: &Path,
    published: io::Result<()>,
    invalidate: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<bool> {
    if published.is_ok() {
        return Ok(true);
    }
    match invalidate(path) {
        Ok(()) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Atomically replaces a bundle with a fresh file readable only by its owner on Unix.
fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    create_private_dir(
        path.parent()
            .ok_or_else(|| io::Error::other("missing trust directory"))?,
    )?;
    let temporary = path.with_extension("tmp");
    match fs::remove_file(&temporary) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    let written = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_worlds::test_support::scratch;

    #[test]
    fn a_refresh_replaces_previous_trust_privately() {
        let root = scratch();
        assert!(refresh(root.path(), Some(b"old trust")).unwrap());
        assert!(refresh(root.path(), Some(b"current trust")).unwrap());
        let path = android_server_trust_path(root.path());
        assert_eq!(fs::read(&path).unwrap(), b"current trust");
        assert!(!path.with_extension("tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn unavailable_or_empty_exports_remove_previous_trust() {
        let root = scratch();
        for export in [None, Some(&b""[..])] {
            assert!(refresh(root.path(), Some(b"old trust")).unwrap());
            assert!(!refresh(root.path(), export).unwrap());
            assert!(!android_server_trust_path(root.path()).exists());
        }
        assert!(!refresh(root.path(), None).unwrap());
    }

    #[test]
    fn a_failed_refresh_and_invalidation_refuses_startup_with_readable_old_trust() {
        let root = scratch();
        assert!(refresh(root.path(), Some(b"old trust")).unwrap());
        let path = android_server_trust_path(root.path());
        let denied = || io::Error::from(io::ErrorKind::PermissionDenied);
        let result = finish_refresh(&path, Err(denied()), |_| Err(denied()));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read(path).unwrap(), b"old trust");
    }
}
