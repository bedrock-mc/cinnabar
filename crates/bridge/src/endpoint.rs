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

#[cfg(any(unix, test))]
const UNIX_ENDPOINT_NAME: &str = "game.sock";
#[cfg(windows)]
const WINDOWS_ENDPOINT_NAME: &str = "game.addr";

pub(crate) enum PlatformStream {
    #[cfg(unix)]
    Unix(UnixStream),
    #[cfg(windows)]
    Tcp(TcpStream),
}

#[cfg(any(unix, test))]
const MAX_UNIX_ENDPOINT_PATH_BYTES: usize = 103;

#[cfg(any(unix, test))]
fn clean_unix_path_bytes(path: &[u8]) -> Vec<u8> {
    let rooted = path.first() == Some(&b'/');
    let mut components: Vec<&[u8]> = Vec::new();
    for component in path.split(|byte| *byte == b'/') {
        if component.is_empty() || component == b"." {
            continue;
        }
        if component == b".." {
            if components.last().is_some_and(|previous| *previous != b"..") {
                components.pop();
            } else if !rooted {
                components.push(component);
            }
            continue;
        }
        components.push(component);
    }

    let mut clean = Vec::with_capacity(path.len());
    if rooted {
        clean.push(b'/');
    }
    for component in components {
        if !clean.is_empty() && clean.last() != Some(&b'/') {
            clean.push(b'/');
        }
        clean.extend_from_slice(component);
    }
    if clean.is_empty() {
        clean.push(b'.');
    }
    clean
}

#[cfg(any(unix, test))]
fn unix_endpoint_path_bytes(socket_dir: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};

    let mut joined = Vec::with_capacity(socket_dir.len() + 1 + UNIX_ENDPOINT_NAME.len());
    joined.extend_from_slice(socket_dir);
    if !socket_dir.is_empty() {
        joined.push(b'/');
    }
    joined.extend_from_slice(UNIX_ENDPOINT_NAME.as_bytes());
    let direct = clean_unix_path_bytes(&joined);
    if direct.len() <= MAX_UNIX_ENDPOINT_PATH_BYTES {
        return direct;
    }
    let digest = format!("{:x}", Sha256::digest(&direct));
    format!("/tmp/cinnabar-{}.sock", &digest[..32]).into_bytes()
}

pub(crate) fn endpoint_path(socket_dir: &Path) -> std::path::PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        std::ffi::OsString::from_vec(unix_endpoint_path_bytes(socket_dir.as_os_str().as_bytes()))
            .into()
    }

    #[cfg(windows)]
    {
        socket_dir.join(WINDOWS_ENDPOINT_NAME)
    }
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

    let path = endpoint_path(socket_dir);
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

    #[cfg(unix)]
    #[test]
    fn long_unix_socket_directory_uses_stable_length_safe_endpoint() {
        use std::os::unix::ffi::OsStrExt;

        let directory = Path::new("/var/folders/zz").join("macos-runner-segment-".repeat(8));
        let first = super::endpoint_path(&directory);
        let second = super::endpoint_path(&directory);

        assert_eq!(first, second);
        assert_eq!(
            first,
            Path::new("/tmp/cinnabar-7b260d1b166f7db809ce8c3d8bd42d1a.sock")
        );
        assert!(first.starts_with("/tmp/cinnabar-"));
        assert!(first.as_os_str().as_bytes().len() <= 103);
    }

    #[test]
    fn unix_endpoint_lexical_normalization_matches_go() {
        let repeated_parent = format!("/tmp/{}", "segment/../".repeat(12));
        let mut invalid_bytes = b"/tmp/\xff/".to_vec();
        invalid_bytes.extend(std::iter::repeat_n(b'x', 100));
        let vectors: Vec<(Vec<u8>, Vec<u8>)> = vec![
            (
                b"/tmp//alpha/./beta/../gamma".to_vec(),
                b"/tmp/alpha/gamma/game.sock".to_vec(),
            ),
            (repeated_parent.into_bytes(), b"/tmp/game.sock".to_vec()),
            (
                format!("/{}", "a".repeat(92)).into_bytes(),
                format!("/{}/game.sock", "a".repeat(92)).into_bytes(),
            ),
            (
                format!("/{}", "a".repeat(93)).into_bytes(),
                b"/tmp/cinnabar-d32a5982698ad8de34829c65f893edf6.sock".to_vec(),
            ),
            (
                format!("/tmp/{}", "路径/".repeat(20)).into_bytes(),
                b"/tmp/cinnabar-08390d1ff13834e20abadae40eff1ce0.sock".to_vec(),
            ),
            (
                invalid_bytes,
                b"/tmp/cinnabar-32ec4a93b88918d1547cfbaf69f63a13.sock".to_vec(),
            ),
        ];

        for (socket_dir, expected) in vectors {
            assert_eq!(super::unix_endpoint_path_bytes(&socket_dir), expected);
        }
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
