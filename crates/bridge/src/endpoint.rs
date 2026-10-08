use std::io;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
#[cfg(any(windows, test))]
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;

use crate::BridgeError;

#[cfg(any(unix, test))]
const GAME_UNIX_ENDPOINT_NAME: &str = "game.sock";
#[cfg(unix)]
const CONTROL_UNIX_ENDPOINT_NAME: &str = "control.sock";
#[cfg(windows)]
const GAME_WINDOWS_ENDPOINT_NAME: &str = "game.addr";
#[cfg(windows)]
const CONTROL_WINDOWS_ENDPOINT_NAME: &str = "control.addr";
#[cfg(any(windows, test))]
const MAX_WINDOWS_PUBLICATION_BYTES: usize = 128;
/// Lets one large batch cross the local Unix socket in a single write; macOS defaults to 8 KiB,
/// which splits it and delays the frames queued behind it.
#[cfg(unix)]
const LOCAL_SOCKET_BUFFER_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum EndpointKind {
    Game,
    Control,
}

impl EndpointKind {
    #[cfg(unix)]
    const fn unix_name(self) -> &'static str {
        match self {
            Self::Game => GAME_UNIX_ENDPOINT_NAME,
            Self::Control => CONTROL_UNIX_ENDPOINT_NAME,
        }
    }

    #[cfg(windows)]
    const fn windows_name(self) -> &'static str {
        match self {
            Self::Game => GAME_WINDOWS_ENDPOINT_NAME,
            Self::Control => CONTROL_WINDOWS_ENDPOINT_NAME,
        }
    }
}

pub(crate) enum PlatformStream {
    #[cfg(unix)]
    Unix(UnixStream),
    #[cfg(windows)]
    Tcp(TcpStream),
}

pub(crate) enum PlatformReadHalf {
    #[cfg(unix)]
    Unix(tokio::net::unix::OwnedReadHalf),
    #[cfg(windows)]
    Tcp(tokio::net::tcp::OwnedReadHalf),
}

pub(crate) enum PlatformWriteHalf {
    #[cfg(unix)]
    Unix(tokio::net::unix::OwnedWriteHalf),
    #[cfg(windows)]
    Tcp(tokio::net::tcp::OwnedWriteHalf),
}

impl PlatformStream {
    pub(crate) fn into_split(self) -> (PlatformReadHalf, PlatformWriteHalf) {
        match self {
            #[cfg(unix)]
            Self::Unix(stream) => {
                let (read, write) = stream.into_split();
                (PlatformReadHalf::Unix(read), PlatformWriteHalf::Unix(write))
            }
            #[cfg(windows)]
            Self::Tcp(stream) => {
                let (read, write) = stream.into_split();
                (PlatformReadHalf::Tcp(read), PlatformWriteHalf::Tcp(write))
            }
        }
    }
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
fn unix_endpoint_path_bytes(socket_dir: &[u8], endpoint_name: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};

    let mut joined = Vec::with_capacity(socket_dir.len() + 1 + endpoint_name.len());
    joined.extend_from_slice(socket_dir);
    if !socket_dir.is_empty() {
        joined.push(b'/');
    }
    joined.extend_from_slice(endpoint_name.as_bytes());
    let direct = clean_unix_path_bytes(&joined);
    if direct.len() <= MAX_UNIX_ENDPOINT_PATH_BYTES {
        return direct;
    }
    let digest = format!("{:x}", Sha256::digest(&direct));
    format!("/tmp/cinnabar-{}.sock", &digest[..32]).into_bytes()
}

pub(crate) fn endpoint_path(socket_dir: &Path, kind: EndpointKind) -> std::path::PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        std::ffi::OsString::from_vec(unix_endpoint_path_bytes(
            socket_dir.as_os_str().as_bytes(),
            kind.unix_name(),
        ))
        .into()
    }

    #[cfg(windows)]
    {
        socket_dir.join(kind.windows_name())
    }
}

pub(crate) async fn connect(
    socket_dir: &Path,
    kind: EndpointKind,
) -> Result<PlatformStream, BridgeError> {
    validate_socket_dir(socket_dir)?;

    #[cfg(unix)]
    {
        connect_unix(socket_dir, kind).await
    }

    #[cfg(windows)]
    {
        connect_windows(socket_dir, kind).await
    }
}

fn validate_socket_dir(socket_dir: &Path) -> Result<(), BridgeError> {
    if socket_dir.as_os_str().is_empty() {
        return Err(invalid_endpoint(socket_dir, "socket directory is empty"));
    }
    Ok(())
}

#[cfg(unix)]
async fn connect_unix(
    socket_dir: &Path,
    kind: EndpointKind,
) -> Result<PlatformStream, BridgeError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let path = endpoint_path(socket_dir, kind);
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
    let stream = connect_unix_stream(&path).await.map_err(BridgeError::Io)?;
    Ok(PlatformStream::Unix(stream))
}

/// Connects, then enlarges the socket buffers as far as the kernel allows; a kernel ceiling
/// below the target never fails the connection.
#[cfg(unix)]
async fn connect_unix_stream(path: &Path) -> io::Result<UnixStream> {
    use rustix::net::sockopt::{
        set_socket_recv_buffer_size, set_socket_send_buffer_size, socket_recv_buffer_size,
        socket_send_buffer_size,
    };

    let stream = UnixStream::connect(path).await?;
    let send = best_effort_buffer_size(
        |size| set_socket_send_buffer_size(&stream, size).map_err(io::Error::from),
        socket_send_buffer_size(&stream).unwrap_or(0),
    );
    let receive = best_effort_buffer_size(
        |size| set_socket_recv_buffer_size(&stream, size).map_err(io::Error::from),
        socket_recv_buffer_size(&stream).unwrap_or(0),
    );
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| tracing::debug!(?send, ?receive, "local socket buffer sizes"));
    Ok(stream)
}

/// Applies the largest size from the target, halving down to `default`, that `set` accepts.
#[cfg(unix)]
fn best_effort_buffer_size(
    mut set: impl FnMut(usize) -> io::Result<()>,
    default: usize,
) -> Option<usize> {
    let mut size = LOCAL_SOCKET_BUFFER_BYTES;
    while size > default {
        if set(size).is_ok() {
            return Some(size);
        }
        size /= 2;
    }
    None
}

#[cfg(windows)]
async fn connect_windows(
    socket_dir: &Path,
    kind: EndpointKind,
) -> Result<PlatformStream, BridgeError> {
    let path = socket_dir.join(kind.windows_name());
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|source| endpoint_read(&path, source))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid_endpoint(
            &path,
            "endpoint publication is not a regular file",
        ));
    }
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|source| endpoint_read(&path, source))?;
    let publication = read_windows_publication(file)
        .await
        .map_err(|source| endpoint_read(&path, source))?;
    let address = parse_windows_publication(&path, &publication)?;
    let stream = connect_loopback_tcp(address)
        .await
        .map_err(BridgeError::Io)?;
    Ok(PlatformStream::Tcp(stream))
}

/// Every frame is a whole batch, so Nagle would only hold a burst's second frame for an ACK.
#[cfg(any(windows, test))]
async fn connect_loopback_tcp(address: std::net::SocketAddrV4) -> io::Result<TcpStream> {
    let stream = TcpStream::connect(address).await?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// Reads an endpoint publication before validating its canonical address.
#[cfg(any(windows, test))]
async fn read_windows_publication(reader: impl AsyncRead + Unpin) -> io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    reader
        .take((MAX_WINDOWS_PUBLICATION_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    Ok(bytes)
}

#[cfg(windows)]
fn parse_windows_publication(
    path: &Path,
    publication: &[u8],
) -> Result<std::net::SocketAddrV4, BridgeError> {
    use std::net::{Ipv4Addr, SocketAddrV4};

    if !(2..=MAX_WINDOWS_PUBLICATION_BYTES).contains(&publication.len())
        || publication.last() != Some(&b'\n')
    {
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

impl AsyncRead for PlatformReadHalf {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(half) => Pin::new(half).poll_read(cx, buffer),
            #[cfg(windows)]
            Self::Tcp(half) => Pin::new(half).poll_read(cx, buffer),
        }
    }
}

impl AsyncWrite for PlatformWriteHalf {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(half) => Pin::new(half).poll_write(cx, buffer),
            #[cfg(windows)]
            Self::Tcp(half) => Pin::new(half).poll_write(cx, buffer),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(half) => Pin::new(half).poll_flush(cx),
            #[cfg(windows)]
            Self::Tcp(half) => Pin::new(half).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(half) => Pin::new(half).poll_shutdown(cx),
            #[cfg(windows)]
            Self::Tcp(half) => Pin::new(half).poll_shutdown(cx),
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

    #[tokio::test]
    async fn review_endpoint_publication_read_is_bounded_before_allocation() {
        let mut reader = std::io::Cursor::new(vec![b'x'; 1024 * 1024]);
        let bytes = super::read_windows_publication(&mut reader).await.unwrap();
        assert_eq!(bytes.len(), super::MAX_WINDOWS_PUBLICATION_BYTES + 1);
        assert_eq!(reader.position(), bytes.len() as u64);
    }

    #[tokio::test]
    async fn loopback_tcp_connection_disables_nagle() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let std::net::SocketAddr::V4(address) = listener.local_addr().unwrap() else {
            unreachable!("bound to an IPv4 loopback address")
        };

        let stream = super::connect_loopback_tcp(address).await.unwrap();

        assert!(stream.nodelay().unwrap());
    }

    /// The client end must not keep macOS's 8 KiB Unix socket buffers.
    #[cfg(unix)]
    #[tokio::test]
    async fn unix_connection_uses_large_socket_buffers() {
        use rustix::net::sockopt::{socket_recv_buffer_size, socket_send_buffer_size};

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("game.sock");
        let _listener = tokio::net::UnixListener::bind(&path).unwrap();
        let untuned = tokio::net::UnixStream::connect(&path).await.unwrap();

        let tuned = super::connect_unix_stream(&path).await.unwrap();

        for read in [
            socket_send_buffer_size::<&tokio::net::UnixStream>,
            socket_recv_buffer_size,
        ] {
            let size = read(&tuned).unwrap();
            assert!(size > read(&untuned).unwrap());
            #[cfg(target_os = "macos")]
            assert!(size >= super::LOCAL_SOCKET_BUFFER_BYTES);
        }
    }

    /// A kernel that rejects large buffers gets the largest size it accepts, or none at all,
    /// and never an error.
    #[cfg(unix)]
    #[test]
    fn rejected_buffer_sizes_fall_back_without_failing() {
        let ceiling = 1024 * 1024;
        let mut attempts = Vec::new();
        let applied = super::best_effort_buffer_size(
            |size| {
                attempts.push(size);
                if size > ceiling {
                    Err(std::io::Error::from_raw_os_error(55))
                } else {
                    Ok(())
                }
            },
            8 * 1024,
        );
        assert_eq!(applied, Some(ceiling));
        assert_eq!(attempts, [4 * ceiling, 2 * ceiling, ceiling]);

        let none = super::best_effort_buffer_size(
            |_| Err(std::io::Error::from_raw_os_error(55)),
            8 * 1024,
        );
        assert_eq!(none, None);
    }

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
        let first = super::endpoint_path(&directory, super::EndpointKind::Game);
        let second = super::endpoint_path(&directory, super::EndpointKind::Game);

        assert_eq!(first, second);
        assert_eq!(
            first,
            Path::new("/tmp/cinnabar-7b260d1b166f7db809ce8c3d8bd42d1a.sock")
        );
        assert_eq!(first.parent(), Some(Path::new("/tmp")));
        let file_name = first
            .file_name()
            .expect("length-safe endpoint must have a filename")
            .as_bytes();
        assert!(file_name.starts_with(b"cinnabar-"));
        assert!(file_name.ends_with(b".sock"));
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
            assert_eq!(
                super::unix_endpoint_path_bytes(&socket_dir, super::GAME_UNIX_ENDPOINT_NAME),
                expected
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn control_endpoint_is_distinct_and_length_safe() {
        let direct = Path::new("/tmp/cinnabar-control-test");
        assert_eq!(
            super::endpoint_path(direct, super::EndpointKind::Control),
            direct.join("control.sock")
        );

        let directory = Path::new("/var/folders/zz").join("macos-runner-segment-".repeat(8));
        let game = super::endpoint_path(&directory, super::EndpointKind::Game);
        let control = super::endpoint_path(&directory, super::EndpointKind::Control);
        assert_ne!(game, control);
        assert!(control.to_string_lossy().starts_with("/tmp/cinnabar-"));
        assert!(control.to_string_lossy().ends_with(".sock"));
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
