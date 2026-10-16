use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use bytes::{Buf, BufMut, Bytes, BytesMut};
#[cfg(not(target_arch = "wasm32"))]
use futures::Sink;
use futures::{SinkExt, Stream};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
#[cfg(not(target_arch = "wasm32"))]
use tokio_util::codec::Framed;
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};

use crate::BridgeError;
#[cfg(not(target_arch = "wasm32"))]
use crate::endpoint::{PlatformReadHalf, PlatformStream};
#[cfg(target_arch = "wasm32")]
use tokio::io::Empty as PlatformReadHalf;

/// Shared u32 big-endian framing, with payload bounds checked before assembly grows.
pub struct BridgeCodec {
    expected_payload: Option<usize>,
    maximum: usize,
}

impl BridgeCodec {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_max(crate::MAX_FRAME_LEN)
    }

    /// Creates a codec that admits only nonempty payloads up to `maximum` bytes.
    pub fn with_max(maximum: usize) -> Self {
        Self {
            expected_payload: None,
            maximum,
        }
    }
}

impl Decoder for BridgeCodec {
    type Item = Bytes;
    type Error = BridgeError;

    fn decode(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if self.expected_payload.is_none() {
            if source.len() < 4 {
                return Ok(None);
            }
            let length = u32::from_be_bytes(source[..4].try_into().expect("four-byte header"));
            let length = length as usize;
            validate_frame_length(length, self.maximum)?;
            self.expected_payload = Some(length);
            source.advance(4);
        }

        let length = self.expected_payload.expect("validated header");
        if source.len() < length {
            return Ok(None);
        }
        self.expected_payload = None;
        Ok(Some(source.split_to(length).freeze()))
    }

    fn decode_eof(&mut self, source: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(frame) = self.decode(source)? {
            return Ok(Some(frame));
        }
        if let Some(expected) = self.expected_payload {
            return Err(BridgeError::TruncatedFrame {
                expected,
                received: source.len(),
            });
        }
        if !source.is_empty() {
            return Err(BridgeError::TruncatedFrame {
                expected: 4,
                received: source.len(),
            });
        }
        Ok(None)
    }
}

impl Encoder<Bytes> for BridgeCodec {
    type Error = BridgeError;

    fn encode(&mut self, item: Bytes, destination: &mut BytesMut) -> Result<(), Self::Error> {
        validate_frame_length(item.len(), self.maximum)?;
        let length = u32::try_from(item.len()).map_err(|_| BridgeError::FrameTooLarge {
            length: item.len(),
            maximum: u32::MAX as usize,
        })?;
        destination.put_u32(length);
        destination.extend_from_slice(&item);
        Ok(())
    }
}

fn validate_frame_length(length: usize, maximum: usize) -> Result<(), BridgeError> {
    if length == 0 {
        return Err(BridgeError::ZeroLengthFrame);
    }
    if length > maximum {
        return Err(BridgeError::FrameTooLarge { length, maximum });
    }
    Ok(())
}

/// A local byte stream framed as unsigned 32-bit big-endian payloads.
#[cfg(not(target_arch = "wasm32"))]
pub struct FramedStream {
    inner: Framed<PlatformStream, BridgeCodec>,
}

#[cfg(not(target_arch = "wasm32"))]
impl FramedStream {
    pub(crate) fn with_max(stream: PlatformStream, maximum: usize) -> Self {
        Self {
            inner: Framed::new(stream, BridgeCodec::with_max(maximum)),
        }
    }
}

/// Frames accepted but not yet written; a stalled reader backs up into the senders, not memory.
const QUEUED_FRAMES: usize = 16;

/// Splits a fresh stream so a spawned writer task owns its write half.
///
/// Requires a Tokio runtime. The writer exits once every [`FrameQueue`] clone is dropped.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn queued(stream: PlatformStream, maximum: usize) -> (FramedReader, FrameQueue) {
    let (read, write) = stream.into_split();
    let trace_conn = crate::latency_trace::active().map(|_| Arc::new(AtomicU64::new(0)));
    let (reader, frames, writer) = prepare_bounded(read, write, maximum, None, trace_conn);
    tokio::spawn(writer);
    (reader, frames)
}

/// Builds the shared framing task without choosing a host executor or stream implementation.
pub(crate) fn prepare<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    read: R,
    write: W,
    maximum: usize,
) -> (FramedReader<R>, FrameQueue, impl Future<Output = ()>) {
    prepare_bounded(read, write, maximum, None, None)
}

/// Keeps byte permits until flush, admitting a frame larger than the budget alone. A `trace_conn`
/// records this connection's batch frames in the latency trace once the core's ID is set in it.
pub(crate) fn prepare_bounded<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    read: R,
    write: W,
    maximum: usize,
    budget: Option<usize>,
    trace_conn: Option<Arc<AtomicU64>>,
) -> (FramedReader<R>, FrameQueue, impl Future<Output = ()>) {
    assert!(budget.is_none_or(|bytes| bytes > 0 && bytes <= u32::MAX as usize));
    let (frames, queued) = mpsc::channel(QUEUED_FRAMES);
    let failure = Arc::new(OnceLock::new());
    let writer = write_queued_frames(
        FramedWrite::new(write, BridgeCodec::with_max(maximum)),
        queued,
        Arc::clone(&failure),
        FrameTrace::new(trace_conn.clone()),
    );
    (
        FramedReader {
            inner: FramedRead::new(read, BridgeCodec::with_max(maximum)),
            trace: FrameTrace::new(trace_conn),
        },
        FrameQueue {
            frames,
            failure,
            maximum,
            budget: budget.map(|bytes| (Arc::new(Semaphore::new(bytes)), bytes)),
        },
        writer,
    )
}

/// The read half of a queued game connection. Browser callers supply their reader type;
/// native callers use the default socket read half.
pub struct FramedReader<R = PlatformReadHalf> {
    inner: FramedRead<R, BridgeCodec>,
    trace: FrameTrace,
}

impl<R> FramedReader<R> {
    /// The core's latency-trace ID for this connection, or 0 when it is not traced.
    #[must_use]
    pub fn trace_conn(&self) -> u64 {
        self.trace.id()
    }

    /// Adopts the latency-trace ID the core sent in its handoff, for this reader and its writer.
    pub fn set_trace_conn(&self, id: u64) {
        if let Some(conn) = &self.trace.conn {
            conn.store(id, Ordering::Relaxed);
        }
    }
}

impl<R: AsyncRead + Unpin> Stream for FramedReader<R> {
    type Item = Result<Bytes, BridgeError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let next = Pin::new(&mut this.inner).poll_next(cx);
        if let Poll::Ready(Some(Ok(frame))) = &next {
            let read_at = this.trace.now();
            this.trace.batch(
                crate::latency_trace::kind::FRAME_READ,
                read_at,
                frame.first(),
                frame.len(),
            );
        }
        next
    }
}

/// Numbers one direction's batch frames in the latency trace; inert until the core's ID is set.
struct FrameTrace {
    trace: Option<&'static crate::latency_trace::Trace>,
    conn: Option<Arc<AtomicU64>>, // shared by a connection's reader and writer
    frames: u64,
}

impl FrameTrace {
    fn new(conn: Option<Arc<AtomicU64>>) -> Self {
        let trace = conn.as_ref().and(crate::latency_trace::active());
        Self {
            trace,
            conn,
            frames: 0,
        }
    }

    fn id(&self) -> u64 {
        self.conn
            .as_ref()
            .map_or(0, |conn| conn.load(Ordering::Relaxed))
    }

    /// The trace clock now, or 0 when this connection is not traced.
    fn now(&self) -> i64 {
        self.trace.map_or(0, |trace| trace.now())
    }

    /// Records a frame of `len` bytes starting with `head` under `kind`, stamped `at`, when it is a
    /// batch on a traced connection.
    fn batch(&mut self, kind: u8, at: i64, head: Option<&u8>, len: usize) {
        let conn = self.id();
        if conn == 0 || head != Some(&crate::contract::KIND_BATCH) {
            return;
        }
        let Some(trace) = self.trace else {
            return;
        };
        self.frames += 1;
        trace.record(kind, &[conn as i64, self.frames as i64, at, len as i64]);
    }
}

struct QueuedFrame {
    frame: Bytes,
    written: oneshot::Sender<Result<(), String>>,
    bytes: Option<OwnedSemaphorePermit>,
}

/// Queues whole frames for one connection's writer task; every clone shares one bounded FIFO.
#[derive(Clone)]
pub struct FrameQueue {
    frames: mpsc::Sender<QueuedFrame>,
    failure: Arc<OnceLock<String>>,
    maximum: usize,
    budget: Option<(Arc<Semaphore>, usize)>,
}

/// Resolves once an accepted frame has been flushed to the socket.
pub struct FrameWritten {
    written: oneshot::Receiver<Result<(), String>>,
    failure: Arc<OnceLock<String>>,
}

impl Future for FrameWritten {
    type Output = Result<(), BridgeError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let failure = Arc::clone(&self.failure);
        Pin::new(&mut self.written)
            .poll(cx)
            .map(|result| match result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(reason)) => Err(broken_pipe(&reason)),
                Err(_) => Err(writer_stopped(&failure)),
            })
    }
}

impl FrameQueue {
    /// Writes `frame` after every frame accepted before it and resolves once it is flushed.
    pub async fn send(&self, frame: Bytes) -> Result<(), BridgeError> {
        self.accept(frame).await?.await
    }

    /// Waits for queue capacity, then accepts `frame`; the returned future tracks its write.
    pub async fn accept(&self, frame: Bytes) -> Result<FrameWritten, BridgeError> {
        validate_frame_length(frame.len(), self.maximum)?;
        let bytes = if let Some((semaphore, budget)) = &self.budget {
            Some(
                Arc::clone(semaphore)
                    .acquire_many_owned(frame.len().min(*budget) as u32)
                    .await
                    .map_err(|_| writer_stopped(&self.failure))?,
            )
        } else {
            None
        };
        let permit = self
            .frames
            .reserve()
            .await
            .map_err(|_| writer_stopped(&self.failure))?;
        let (written, receiver) = oneshot::channel();
        permit.send(QueuedFrame {
            frame,
            written,
            bytes,
        });
        Ok(FrameWritten {
            written: receiver,
            failure: Arc::clone(&self.failure),
        })
    }
}

fn writer_stopped(failure: &OnceLock<String>) -> BridgeError {
    broken_pipe(
        failure
            .get()
            .map_or("bridge writer stopped", String::as_str),
    )
}

fn broken_pipe(reason: &str) -> BridgeError {
    BridgeError::Io(io::Error::new(io::ErrorKind::BrokenPipe, reason.to_owned()))
}

/// Writes queued frames in order, coalescing whatever is already queued into one flush, and
/// acknowledges each frame only after that flush.
async fn write_queued_frames<W: AsyncWrite + Unpin>(
    mut writer: FramedWrite<W, BridgeCodec>,
    mut frames: mpsc::Receiver<QueuedFrame>,
    failure: Arc<OnceLock<String>>,
    mut trace: FrameTrace,
) {
    let mut pending = Vec::with_capacity(QUEUED_FRAMES);
    // First byte, length and feed time of the frames in this flush, kept only while tracing.
    // Each time is taken before that frame's bytes can go out, so the core never reads a frame
    // before its record.
    let mut traced = Vec::new();
    while let Some(first) = frames.recv().await {
        if trace.conn.is_some() {
            traced.push((first.frame.first().copied(), first.frame.len(), trace.now()));
        }
        let mut written = writer.feed(first.frame).await;
        pending.push((first.written, first.bytes));
        while written.is_ok()
            && pending.len() < QUEUED_FRAMES
            && let Ok(next) = frames.try_recv()
        {
            if trace.conn.is_some() {
                traced.push((next.frame.first().copied(), next.frame.len(), trace.now()));
            }
            written = writer.feed(next.frame).await;
            pending.push((next.written, next.bytes));
        }
        let flushed = match written {
            Ok(()) => writer.flush().await,
            Err(error) => Err(error),
        };
        let result = flushed.map_err(|error| error.to_string());
        if let Err(reason) = &result {
            let _ = failure.set(reason.clone());
        }
        // Only frames that reached the socket count as written; a failure ends the connection.
        for (head, len, fed) in traced.drain(..) {
            if result.is_ok() {
                trace.batch(
                    crate::latency_trace::kind::FRAME_WRITTEN,
                    fed,
                    head.as_ref(),
                    len,
                );
            }
        }
        for (written, _bytes) in pending.drain(..) {
            let _ = written.send(result.clone());
        }
        if result.is_err() {
            return;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Stream for FramedStream {
    type Item = Result<Bytes, BridgeError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().inner).poll_next(cx)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Sink<Bytes> for FramedStream {
    type Error = BridgeError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner).poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        Pin::new(&mut self.get_mut().inner).start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner).poll_close(cx)
    }
}

#[cfg(test)]
mod tests {
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
}
