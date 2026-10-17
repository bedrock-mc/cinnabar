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
use crate::contract::QUEUED_FRAMES;
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

/// Ordered bridge frames and their shared latency-trace identity. Implementations retain
/// partial input when a receive future is cancelled.
pub trait FrameRead: Stream<Item = Result<Bytes, BridgeError>> + Unpin {
    /// Returns the core's connection identity, or zero when tracing is disabled.
    fn trace_conn(&self) -> u64;
    /// Shares the handed-off identity with this reader's writer.
    fn set_trace_conn(&self, id: u64);
}

impl<R: AsyncRead + Unpin> FrameRead for FramedReader<R> {
    fn trace_conn(&self) -> u64 {
        self.trace_conn()
    }

    fn set_trace_conn(&self, id: u64) {
        self.set_trace_conn(id);
    }
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
mod tests;
