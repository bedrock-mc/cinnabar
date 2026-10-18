use bytes::{BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use super::{BridgeCodec, validate_frame_length};
use crate::{BridgeError, MAX_FRAME_LEN};

/// Accepted streams use the same kernel-limited buffers as connecting streams.
#[cfg(unix)]
#[tokio::test]
async fn accepted_unix_stream_uses_tuned_buffers() {
    use crate::endpoint::{EndpointKind, PlatformStream};
    use rustix::net::sockopt::{socket_recv_buffer_size, socket_send_buffer_size};

    let directory = tempfile::tempdir().unwrap();
    let listener = crate::SessionListener::bind(directory.path())
        .await
        .unwrap();
    let PlatformStream::Unix(client) =
        crate::endpoint::connect(directory.path(), EndpointKind::Session)
            .await
            .unwrap();
    let accepted = listener.accept().await.unwrap();
    let PlatformStream::Unix(server) = accepted.inner.get_ref();
    for read in [
        socket_send_buffer_size::<&tokio::net::UnixStream>,
        socket_recv_buffer_size,
    ] {
        assert_eq!(read(server).unwrap(), read(&client).unwrap());
    }
}

/// Clones of the queue share one writer that keeps the order frames were accepted in,
/// while the read half keeps receiving.
#[cfg(unix)]
#[tokio::test]
async fn queued_connection_writes_in_acceptance_order_while_reading() {
    use futures::StreamExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (local, mut peer) = tokio::net::UnixStream::pair().unwrap();
    let (mut reader, queue) =
        super::queued(crate::endpoint::PlatformStream::Unix(local), MAX_FRAME_LEN);
    peer.write_all(&wire_frame(&[2])).await.unwrap();
    let session = queue.clone();
    let first = queue.accept(Bytes::from_static(&[3])).await.unwrap();
    let second = session.accept(Bytes::from_static(&[4])).await.unwrap();
    let third = queue.accept(Bytes::from_static(&[5])).await.unwrap();

    assert_eq!(&reader.next().await.unwrap().unwrap()[..], [2]);
    let mut written = [0; 15];
    peer.read_exact(&mut written).await.unwrap();
    let mut expected = wire_frame(&[3]);
    expected.extend_from_slice(&wire_frame(&[4]));
    expected.extend_from_slice(&wire_frame(&[5]));
    assert_eq!(&written[..], &expected[..]);
    for written in [first, second, third] {
        written.await.unwrap();
    }
}

/// A send completes only once its frame is flushed, and fails when the write does.
#[cfg(unix)]
#[tokio::test]
async fn send_resolves_only_after_its_frame_is_flushed() {
    use futures::FutureExt;
    use tokio::io::AsyncReadExt;

    let (local, mut peer) = tokio::net::UnixStream::pair().unwrap();
    let (_reader, queue) =
        super::queued(crate::endpoint::PlatformStream::Unix(local), MAX_FRAME_LEN);
    let payload = Bytes::from(vec![7; 4 * 1024 * 1024]);
    let mut send = Box::pin(queue.send(payload.clone()));
    for _ in 0..64 {
        assert!(
            (&mut send).now_or_never().is_none(),
            "a frame the peer has not drained must not report written"
        );
        tokio::task::yield_now().await;
    }

    let mut drained = vec![0; 4 + payload.len()];
    let (read, sent) = tokio::join!(peer.read_exact(&mut drained), send);
    read.unwrap();
    sent.unwrap();

    drop(peer);
    let mut failed = Ok(());
    for _ in 0..8 {
        failed = queue.send(Bytes::from_static(&[1])).await;
        if failed.is_err() {
            break;
        }
    }
    assert!(failed.is_err(), "a write to a closed peer must fail");
}

/// A reader that stops draining backs up into the senders instead of an unbounded backlog.
#[cfg(unix)]
#[tokio::test]
async fn stalled_reader_bounds_accepted_frames() {
    use futures::FutureExt;

    let (local, _peer) = tokio::net::UnixStream::pair().unwrap();
    let (_reader, queue) =
        super::queued(crate::endpoint::PlatformStream::Unix(local), MAX_FRAME_LEN);
    let frame = Bytes::from(vec![7; 64 * 1024]);
    let mut accepted = Vec::new();
    let mut backed_up = false;
    // 4096 frames of 64 KiB is 256 MiB, far beyond any socket buffer.
    for _ in 0..4096 {
        match queue.accept(frame.clone()).now_or_never() {
            Some(written) => accepted.push(written.unwrap()),
            None => {
                backed_up = true;
                break;
            }
        }
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
    }

    assert!(
        backed_up,
        "accepted {} frames for a stalled reader without backing up",
        accepted.len()
    );
}

fn wire_frame(payload: &[u8]) -> BytesMut {
    let mut wire = BytesMut::with_capacity(4 + payload.len());
    wire.put_u32(payload.len() as u32);
    wire.extend_from_slice(payload);
    wire
}

#[test]
fn encode_writes_u32_big_endian_payload_length() {
    let mut codec = BridgeCodec::new();
    let mut wire = BytesMut::new();

    codec
        .encode(Bytes::from_static(&[0xfe, 0x01]), &mut wire)
        .expect("encode frame");

    assert_eq!(&wire[..], &[0x00, 0x00, 0x00, 0x02, 0xfe, 0x01]);
}

#[test]
fn decode_preserves_fifo_order_and_returns_immutable_bytes() {
    let mut codec = BridgeCodec::new();
    let mut wire = wire_frame(&[0xfe, 0x01]);
    wire.extend_from_slice(&wire_frame(&[0xfe, 0x02]));

    let first: Bytes = codec
        .decode(&mut wire)
        .expect("decode first frame")
        .expect("first frame");
    let second: Bytes = codec
        .decode(&mut wire)
        .expect("decode second frame")
        .expect("second frame");

    assert_eq!(&first[..], &[0xfe, 0x01]);
    assert_eq!(&second[..], &[0xfe, 0x02]);
    assert!(
        codec
            .decode(&mut wire)
            .expect("decode empty buffer")
            .is_none()
    );
}

#[test]
fn decode_rejects_zero_length_frame() {
    let mut codec = BridgeCodec::new();
    let mut wire = BytesMut::from(&[0, 0, 0, 0][..]);

    let error = codec.decode(&mut wire).expect_err("zero frame must fail");

    assert!(matches!(error, BridgeError::ZeroLengthFrame));
}

#[test]
fn encode_rejects_zero_length_frame() {
    let mut codec = BridgeCodec::new();
    let mut wire = BytesMut::new();

    let error = codec
        .encode(Bytes::new(), &mut wire)
        .expect_err("zero frame must fail");

    assert!(matches!(error, BridgeError::ZeroLengthFrame));
    assert!(wire.is_empty());
}

#[test]
fn decode_rejects_oversized_length_before_payload_arrives() {
    let mut codec = BridgeCodec::new();
    let mut wire = BytesMut::new();
    wire.put_u32((MAX_FRAME_LEN + 1) as u32);

    let error = codec
        .decode(&mut wire)
        .expect_err("oversized frame must fail");

    assert!(matches!(
        error,
        BridgeError::FrameTooLarge {
            length,
            maximum
        } if length == MAX_FRAME_LEN + 1 && maximum == MAX_FRAME_LEN
    ));
    assert_eq!(wire.len(), 4);
}

#[test]
fn encode_rejects_oversized_payload() {
    let mut codec = BridgeCodec::new();
    let mut wire = BytesMut::new();
    let payload = Bytes::from(vec![0; MAX_FRAME_LEN + 1]);

    let error = codec
        .encode(payload, &mut wire)
        .expect_err("oversized frame must fail");

    assert!(matches!(
        error,
        BridgeError::FrameTooLarge {
            length,
            maximum
        } if length == MAX_FRAME_LEN + 1 && maximum == MAX_FRAME_LEN
    ));
    assert!(wire.is_empty());
}

#[test]
fn maximum_frame_length_is_accepted() {
    assert!(validate_frame_length(MAX_FRAME_LEN, MAX_FRAME_LEN).is_ok());
}

#[test]
fn endpoint_specific_limit_rejects_a_larger_frame() {
    let mut codec = BridgeCodec::with_max(64 * 1024);
    let mut wire = BytesMut::new();
    wire.put_u32(64 * 1024 + 1);

    let error = codec.decode(&mut wire).expect_err("frame must fail");
    assert!(matches!(
        error,
        BridgeError::FrameTooLarge {
            length: 65_537,
            maximum: 65_536
        }
    ));
}

#[test]
fn decode_eof_rejects_partial_header() {
    let mut codec = BridgeCodec::new();
    let mut wire = BytesMut::from(&[0, 0][..]);

    let error = codec
        .decode_eof(&mut wire)
        .expect_err("partial header must fail");

    assert!(matches!(
        error,
        BridgeError::TruncatedFrame {
            expected: 4,
            received: 2
        }
    ));
}

#[test]
fn decode_eof_rejects_header_without_payload() {
    let mut codec = BridgeCodec::new();
    let mut wire = BytesMut::from(&[0, 0, 0, 3][..]);

    assert!(codec.decode(&mut wire).expect("decode header").is_none());
    assert!(wire.is_empty());
    let error = codec
        .decode_eof(&mut wire)
        .expect_err("missing payload must fail");

    assert!(matches!(
        error,
        BridgeError::TruncatedFrame {
            expected: 3,
            received: 0
        }
    ));
}

#[test]
fn decode_eof_rejects_partial_payload() {
    let mut codec = BridgeCodec::new();
    let mut wire = BytesMut::from(&[0, 0, 0, 3, 0xfe][..]);

    assert!(
        codec
            .decode(&mut wire)
            .expect("decode partial frame")
            .is_none()
    );
    let error = codec
        .decode_eof(&mut wire)
        .expect_err("partial payload must fail");

    assert!(matches!(
        error,
        BridgeError::TruncatedFrame {
            expected: 3,
            received: 1
        }
    ));
}

#[test]
fn decode_eof_between_frames_is_clean() {
    let mut codec = BridgeCodec::new();
    let mut wire = BytesMut::new();

    assert!(codec.decode_eof(&mut wire).expect("clean EOF").is_none());
}
#[test]
fn declared_payload_does_not_allocate_before_bytes_arrive() {
    let mut header = BytesMut::from(&(MAX_FRAME_LEN as u32).to_be_bytes()[..]);
    assert!(
        BridgeCodec::with_max(MAX_FRAME_LEN)
            .decode(&mut header)
            .unwrap()
            .is_none()
    );
    assert!(header.capacity() < MAX_FRAME_LEN);
}

/// A writer whose every write fails, standing in for a closed core socket.
struct FailingWriter;

impl tokio::io::AsyncWrite for FailingWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        _: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

/// Queues `batches` together, so the writer coalesces them into one flush, writes them through
/// the writer `writer` builds and returns the trace lines left.
async fn traced_write<W: tokio::io::AsyncWrite + Unpin>(
    batches: Vec<Bytes>,
    writer: impl FnOnce(&'static crate::latency_trace::Trace) -> W,
) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let trace: &'static _ = Box::leak(Box::new(
        crate::latency_trace::Trace::open(dir.path(), "frames").unwrap(),
    ));
    let write = writer(trace);
    let (frames, queued) = tokio::sync::mpsc::channel(batches.len());
    let mut receipts = Vec::new();
    for frame in batches {
        let (written, receipt) = tokio::sync::oneshot::channel();
        receipts.push(receipt);
        frames
            .send(super::QueuedFrame {
                frame,
                written,
                bytes: None,
            })
            .await
            .unwrap();
    }
    drop(frames);
    let mut frame_trace = super::FrameTrace::new(None);
    frame_trace.trace = Some(trace);
    frame_trace.conn = Some(std::sync::Arc::new(std::sync::atomic::AtomicU64::new(9)));
    super::write_queued_frames(
        tokio_util::codec::FramedWrite::new(write, BridgeCodec::new()),
        queued,
        std::sync::Arc::default(),
        frame_trace,
    )
    .await;
    trace.finish();
    let path = dir
        .path()
        .join(format!("frames-{}.trace", std::process::id()));
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Only a frame that reached the socket is traced as written.
#[tokio::test]
async fn failed_bridge_writes_are_not_traced_as_written() {
    let written = traced_write(vec![small_batch()], |_| tokio::io::sink()).await;
    assert_eq!(written.len(), 1);
    assert!(written[0].starts_with("w 9 1 "));
    assert!(
        traced_write(vec![small_batch()], |_| FailingWriter)
            .await
            .is_empty()
    );
}

/// Stamps the trace clock when the first bytes reach it, as the core's read would.
struct StampingWriter(
    &'static crate::latency_trace::Trace,
    std::sync::Arc<std::sync::atomic::AtomicI64>,
);

impl tokio::io::AsyncWrite for StampingWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let _ = self.1.compare_exchange(
            0,
            self.0.now(),
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
        );
        std::task::Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

/// A written frame's record carries the time its flush began, so the reader cannot observe the
/// bytes before the recorded write.
#[tokio::test]
async fn written_frames_are_stamped_before_their_bytes_leave() {
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0));
    let stamp = std::sync::Arc::clone(&seen);
    let lines = traced_write(vec![small_batch()], |trace| StampingWriter(trace, stamp)).await;
    let recorded: i64 = lines[0].split(' ').nth(3).unwrap().parse().unwrap();
    let seen = seen.load(std::sync::atomic::Ordering::Relaxed);
    assert!(seen != 0, "the writer must have seen the frame");
    assert!(
        recorded <= seen,
        "recorded {recorded} after the bytes left at {seen}"
    );
}

fn small_batch() -> Bytes {
    Bytes::from_static(&[crate::contract::KIND_BATCH, 1])
}

/// Frames coalesced into one flush keep their own times, each taken as that frame is fed, in
/// queue order.
#[tokio::test]
async fn coalesced_frames_keep_their_own_write_times() {
    let large = |fill| {
        let mut frame = vec![fill; 1 << 20];
        frame[0] = crate::contract::KIND_BATCH;
        Bytes::from(frame)
    };
    let lines = traced_write(vec![large(1), large(2)], |_| tokio::io::sink()).await;
    let times: Vec<i64> = lines
        .iter()
        .map(|line| line.split(' ').nth(3).unwrap().parse().unwrap())
        .collect();
    assert_eq!(times.len(), 2);
    assert!(
        times[0] < times[1],
        "the second frame was fed after the first: {times:?}"
    );
}
