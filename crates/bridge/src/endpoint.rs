use std::io;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
#[cfg(windows)]
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;

use crate::BridgeError;

#[cfg(unix)]
const UNIX_ENDPOINT_NAME: &str = "game.sock";
#[cfg(windows)]
const WINDOWS_ENDPOINT_NAME: &str = "game.addr";

pub(crate) enum PlatformStream {
    #[cfg(unix)]
    Unix(UnixStream),
    #[cfg(windows)]
    Tcp(TcpStream),
}

pub(crate) async fn connect(socket_dir: &Path) -> Result<PlatformStream, BridgeError> {
    validate_socket_dir(socket_dir)?;

    #[cfg(unix)]
    {
        connect_unix(socket_dir).await
    }

    #[cfg(windows)]
    {
        connect_windows(socket_dir).await
    }
}

fn validate_socket_dir(socket_dir: &Path) -> Result<(), BridgeError> {
    if socket_dir.as_os_str().is_empty() {
        return Err(invalid_endpoint(socket_dir, "socket directory is empty"));
    }
    Ok(())
}

#[cfg(unix)]
async fn connect_unix(socket_dir: &Path) -> Result<PlatformStream, BridgeError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let path = socket_dir.join(UNIX_ENDPOINT_NAME);
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|source| endpoint_read(&path, source))?;
    if !metadata.file_type().is_socket() {
        return Err(invalid_endpoint(&path, "endpoint is not a Unix socket"));
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(invalid_endpoint(
            &path,
            "Unix socket is not owned by the current user",
        ));
    }
    let stream = UnixStream::connect(&path).await.map_err(BridgeError::Io)?;
    Ok(PlatformStream::Unix(stream))
}

#[cfg(windows)]
async fn connect_windows(socket_dir: &Path) -> Result<PlatformStream, BridgeError> {
    let path = socket_dir.join(WINDOWS_ENDPOINT_NAME);
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|source| endpoint_read(&path, source))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid_endpoint(
            &path,
            "endpoint publication is not a regular file",
        ));
    }
    let publication = tokio::fs::read(&path)
        .await
        .map_err(|source| endpoint_read(&path, source))?;
    let address = parse_windows_publication(&path, &publication)?;
    let stream = TcpStream::connect(address).await.map_err(BridgeError::Io)?;
    Ok(PlatformStream::Tcp(stream))
}

#[cfg(windows)]
fn parse_windows_publication(
    path: &Path,
    publication: &[u8],
) -> Result<std::net::SocketAddrV4, BridgeError> {
    use std::net::{Ipv4Addr, SocketAddrV4};

    if !(2..=128).contains(&publication.len()) || publication.last() != Some(&b'\n') {
        return Err(invalid_endpoint(
            path,
            "publication length or terminator is invalid",
        ));
    }

    let address = &publication[..publication.len() - 1];
    if !address.is_ascii()
        || address
            .iter()
            .any(|byte| byte.is_ascii_whitespace() || *byte == b'\r' || *byte == b'\n')
    {
        return Err(invalid_endpoint(
            path,
            "publication must be canonical ASCII",
        ));
    }

    let address = std::str::from_utf8(address)
        .map_err(|_| invalid_endpoint(path, "publication is not valid ASCII"))?;
    let port_text = address
        .strip_prefix("127.0.0.1:")
        .ok_or_else(|| invalid_endpoint(path, "published host is not 127.0.0.1"))?;
    if port_text.is_empty()
        || !port_text.bytes().all(|byte| byte.is_ascii_digit())
        || (port_text.len() > 1 && port_text.starts_with('0'))
    {
        return Err(invalid_endpoint(path, "published port is not canonical"));
    }
    let port = port_text
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| invalid_endpoint(path, "published port is outside 1..=65535"))?;

    Ok(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

fn endpoint_read(path: &Path, source: io::Error) -> BridgeError {
    BridgeError::EndpointRead {
        path: path.to_path_buf(),
        source,
    }
}

fn invalid_endpoint(path: &Path, reason: impl Into<String>) -> BridgeError {
    BridgeError::InvalidEndpoint {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

impl AsyncRead for PlatformStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_read(cx, buffer),
            #[cfg(windows)]
            Self::Tcp(stream) => Pin::new(stream).poll_read(cx, buffer),
        }
    }
}

impl AsyncWrite for PlatformStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_write(cx, buffer),
            #[cfg(windows)]
            Self::Tcp(stream) => Pin::new(stream).poll_write(cx, buffer),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_flush(cx),
            #[cfg(windows)]
            Self::Tcp(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(stream) => Pin::new(stream).poll_shutdown(cx),
            #[cfg(windows)]
            Self::Tcp(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use std::net::SocketAddrV4;
    use std::path::Path;

    use super::validate_socket_dir;
    use crate::BridgeError;

    #[test]
    fn empty_socket_directory_is_rejected() {
        let error = validate_socket_dir(Path::new("")).expect_err("empty directory must fail");

        assert!(matches!(error, BridgeError::InvalidEndpoint { .. }));
    }

    #[cfg(windows)]
    mod windows {
        use super::*;
        use crate::endpoint::parse_windows_publication;

        #[test]
        fn canonical_publication_parses_to_ipv4_loopback() {
            let path = Path::new("game.addr");
            let address = parse_windows_publication(path, b"127.0.0.1:49152\n")
                .expect("canonical publication");

            assert_eq!(address, "127.0.0.1:49152".parse::<SocketAddrV4>().unwrap());
        }

        #[test]
        fn malformed_publication_bytes_are_rejected() {
            for publication in [
                &b""[..],
                &b"127.0.0.1:80"[..],
                &b"127.0.0.1:80\r\n"[..],
                &b"127.0.0.1:80\n\n"[..],
                &b" 127.0.0.1:80\n"[..],
                &b"127.0.0.1:80 \n"[..],
                &b"127.0.0.1:80\0\n"[..],
                &b"\xef\xbb\xbf127.0.0.1:80\n"[..],
            ] {
                let error = parse_windows_publication(Path::new("game.addr"), publication)
                    .expect_err("malformed publication must fail");
                assert!(matches!(error, BridgeError::InvalidEndpoint { .. }));
            }
        }

        #[test]
        fn noncanonical_or_unsafe_addresses_are_rejected() {
            for publication in [
                &b"localhost:80\n"[..],
                &b"0.0.0.0:80\n"[..],
                &b"127.0.0.1:0\n"[..],
                &b"127.0.0.1:65536\n"[..],
                &b"127.0.0.1:080\n"[..],
                &b"127.0.0.1:+80\n"[..],
            ] {
                let error = parse_windows_publication(Path::new("game.addr"), publication)
                    .expect_err("unsafe publication must fail");
                assert!(matches!(error, BridgeError::InvalidEndpoint { .. }));
            }
        }
    }
}
