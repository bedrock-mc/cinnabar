//! Owns a local session endpoint and its lease for an offline server.

use super::{EndpointKind, PlatformStream};
use crate::{BridgeError, FramedStream, MAX_FRAME_LEN};
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};
#[cfg(windows)]
use tokio::net::TcpListener as Listener;
#[cfg(unix)]
use tokio::net::UnixListener as Listener;

/// A leased endpoint removed on drop only while it is still this listener's publication.
pub struct SessionListener {
    listener: Listener,
    path: PathBuf,
    publication: Publication,
    _lease: File,
}

#[cfg(unix)]
type Publication = (u64, u64);
#[cfg(windows)]
type Publication = Vec<u8>;

impl SessionListener {
    /// Publishes the session endpoint, refusing active listeners and non-endpoint files.
    pub async fn bind(directory: &Path) -> Result<Self, BridgeError> {
        super::validate_socket_dir(directory)?;
        fs::create_dir_all(directory)?;
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() {
            return Err(super::invalid_endpoint(directory, "not a directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.uid() != rustix::process::geteuid().as_raw() {
                return Err(super::invalid_endpoint(
                    directory,
                    "directory is not owned by this user",
                ));
            }
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        }
        let lock_path = directory.join("session.lock");
        if fs::symlink_metadata(&lock_path).is_ok_and(|m| !m.is_file()) {
            return Err(super::invalid_endpoint(
                &lock_path,
                "lease is not a regular file",
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lease = options.open(lock_path)?;
        lease.try_lock().map_err(io::Error::other)?;
        let path = super::endpoint_path(directory, EndpointKind::Session);
        prepare(&path).await?;
        let (listener, publication) = publish(&path).await?;
        Ok(Self {
            listener,
            path,
            publication,
            _lease: lease,
        })
    }

    /// Accepts one framed connection; it may outlive the listener.
    pub async fn accept(&self) -> Result<FramedStream, BridgeError> {
        let (stream, _) = self.listener.accept().await?;
        #[cfg(windows)]
        let stream = {
            stream.set_nodelay(true)?;
            PlatformStream::Tcp(stream)
        };
        #[cfg(unix)]
        let stream = PlatformStream::Unix(stream);
        Ok(FramedStream::with_max(stream, MAX_FRAME_LEN))
    }
}

impl Drop for SessionListener {
    fn drop(&mut self) {
        if identity(&self.path).is_ok_and(|value| value == self.publication) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Inspects an endpoint without following links, retaining its cleanup identity.
#[cfg(unix)]
fn identity(path: &Path) -> Result<Publication, BridgeError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_socket() || metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(super::invalid_endpoint(
            path,
            "endpoint is not an owned socket",
        ));
    }
    Ok((metadata.dev(), metadata.ino()))
}

/// Reads only a regular, bounded loopback publication.
#[cfg(windows)]
fn identity(path: &Path) -> Result<Publication, BridgeError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > super::MAX_WINDOWS_PUBLICATION_BYTES as u64 {
        return Err(super::invalid_endpoint(
            path,
            "invalid endpoint publication",
        ));
    }
    let bytes = fs::read(path)?;
    super::parse_windows_publication(path, &bytes)?;
    Ok(bytes)
}

/// Reclaims only a proven stale endpoint; active or unrecognized files remain intact.
async fn prepare(path: &Path) -> Result<(), BridgeError> {
    if !path.try_exists()? {
        return Ok(());
    }
    let before = identity(path)?;
    #[cfg(unix)]
    let connected = tokio::net::UnixStream::connect(path).await.map(|_| ());
    #[cfg(windows)]
    let connected =
        tokio::net::TcpStream::connect(super::parse_windows_publication(path, &before)?)
            .await
            .map(|_| ());
    match connected {
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
        Err(error) => return Err(error.into()),
        Ok(()) => return Err(super::invalid_endpoint(path, "endpoint is active")),
    }
    if identity(path)? != before {
        return Err(super::invalid_endpoint(path, "endpoint changed"));
    }
    fs::remove_file(path)?;
    Ok(())
}

/// Binds and secures the socket before it can be advertised as ready.
#[cfg(unix)]
async fn publish(path: &Path) -> Result<(Listener, Publication), BridgeError> {
    use std::os::unix::fs::PermissionsExt;
    let listener = Listener::bind(path)?;
    let publication = identity(path)?;
    if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        let _ = fs::remove_file(path);
        return Err(error.into());
    }
    Ok((listener, publication))
}

/// Publishes a bound loopback listener without replacing an existing file.
#[cfg(windows)]
async fn publish(path: &Path) -> Result<(Listener, Publication), BridgeError> {
    use std::io::Write;
    let listener = Listener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let publication = format!("{}\n", listener.local_addr()?).into_bytes();
    let mut file = tempfile::NamedTempFile::new_in(path.parent().expect("endpoint directory"))?;
    file.write_all(&publication)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path).map_err(|error| error.error)?;
    Ok((listener, publication))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lease_blocks_a_second_owner_and_releases_on_drop() {
        let root = tempfile::tempdir().unwrap();
        let listener = SessionListener::bind(root.path()).await.unwrap();
        let path = crate::session_endpoint_path(root.path());
        assert!(path.exists());
        assert!(SessionListener::bind(root.path()).await.is_err());
        drop(listener);
        assert!(!path.exists());
        let next = SessionListener::bind(root.path()).await.unwrap();
        assert!(path.exists());
        drop(next);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn unrelated_endpoint_files_are_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        let path = crate::session_endpoint_path(root.path());
        fs::write(&path, b"not an endpoint").unwrap();
        assert!(SessionListener::bind(root.path()).await.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"not an endpoint");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cleanup_keeps_replacements_and_stale_sockets_can_be_rebound() {
        let root = tempfile::tempdir().unwrap();
        let path = crate::session_endpoint_path(root.path());
        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
        let listener = SessionListener::bind(root.path()).await.unwrap();
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"replacement").unwrap();
        drop(listener);
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
    }
}
